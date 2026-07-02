"""Model-variant benchmark: replay a FIXED spec suite through the swarm and grade by RUNNING.

Why a fixed suite instead of the brain generator: it needs no ANTHROPIC key, and — crucially —
it makes the GGUF vs MLX comparison a clean PAIRED A/B (identical specs both nights) with
replicates per spec, so we measure real run-to-run CONSISTENCY, not brain-generated noise.

Tiers (the operator's ladder), interpreted as (distinct specs) × (replicates):
  smoke   — 1 spec  × 1   = 1 run    (fast end-to-end sanity)
  light   — 3 specs × 1   = 3 runs
  medium  — 3 specs × 5   = 15 runs  (real distributions)
  high    — 3 specs × 10  = 30 runs
  extreme — 3 specs × 10, harder specs (harder-spec pressure is a follow-up)

Each run is tagged with `--variant` (mlx / gguf). Grading is deterministic (judged-by-running:
imports, --help, golden-value commands, pytest) via verify(ev, None) — no brain, no API key.
"""

from __future__ import annotations

import csv
import datetime as dt
import statistics as stats
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

from .collector import collect
from .contracts import DeterministicCheck, TaskSpec
from .ledger import Ledger
from .runner import run_swarm
from . import verifier as verifier_mod

# Per-run hard safety cap (seconds): a genuinely hung swarm run can't eat the whole campaign.
_RUN_CAP_SECS = 3600

# --------------------------------------------------------------------------------------
# The fixed benchmark suite — 3 self-contained apps spanning the swarm's core capability
# axes (CRUD+multi-format, recursive-descent compute, nested transactions). Each carries
# golden-value deterministic checks so "quality" is measured by RUNNING the built app.
# --------------------------------------------------------------------------------------
SUITE: List[Dict[str, Any]] = [
    {
        "id": "crud",
        "bucket": "crud-multiformat",
        "app_slug": "invtrack",
        "language": "python",
        "prompt": (
            "Build a command-line inventory tracker in Python, package name invtrack, run as "
            "python -m invtrack. Persist to SQLite via a GLOBAL --db PATH option that comes BEFORE "
            "the subcommand. Use NESTED subcommands: init; item add SKU --name NAME --qty N --price P "
            "(SKU positional); item get SKU; item list --format FORMAT where FORMAT is one of table "
            "json or csv defaulting to table; item adjust SKU --delta N (change the quantity, may reach "
            "zero but never go below); item delete SKU; report low --threshold N (list SKUs whose qty is "
            "below the threshold); report value (print the total inventory value, the sum of qty*price, "
            "as a single number). Validate with a clear error and a NONZERO exit code for EACH of: a "
            "duplicate SKU on add; an unknown SKU on get, adjust or delete; an adjust that would make qty "
            "negative; an invalid format value; and a negative qty or price on add. Include unit tests for "
            "the json and csv list output, the report value aggregation, the duplicate-SKU rejection, and "
            "the negative-adjust rejection."
        ),
        "checks": [
            {"type": "command_succeeds", "name": "imports", "command": "python3 -m pytest --collect-only -q"},
            {"type": "command_succeeds", "name": "help", "command": "python3 -m invtrack --help"},
            {
                "type": "command_succeeds",
                "name": "golden-value",
                "command": (
                    "python3 -m invtrack --db bench.db init && "
                    "python3 -m invtrack --db bench.db item add A1 --name Bolt --qty 10 --price 2 && "
                    "python3 -m invtrack --db bench.db item add A2 --name Nut --qty 5 --price 1 && "
                    "python3 -m invtrack --db bench.db report value | grep -qE '(^|[^0-9])25([^0-9]|$)'"
                ),
            },
            {
                "type": "command_succeeds",
                "name": "golden-json",
                "command": "python3 -m invtrack --db bench.db item list --format json | python3 -c 'import sys,json; json.load(sys.stdin)'",
            },
            {"type": "command_succeeds", "name": "tests", "command": "python3 -m pytest -q"},
        ],
    },
    {
        "id": "compute",
        "bucket": "compute-parser",
        "app_slug": "rdcalc",
        "language": "python",
        "prompt": (
            "Build a command-line arithmetic expression evaluator in Python, package name rdcalc, run as "
            "python -m rdcalc. Subcommand: eval EXPR where EXPR is a single positional string. Support "
            "integers and decimals, the binary operators + - * /, unary minus, parentheses, and "
            "exponentiation with ^ which is RIGHT-associative, with standard precedence (^ highest, then "
            "* and /, then + and -). Use a real tokenizer and a recursive-descent parser; do NOT use "
            "Python eval or exec. Print the numeric result. Validate with a clear error and a NONZERO exit "
            "code for EACH of: a malformed expression (unbalanced parentheses or a stray operator); "
            "division by zero; and an unknown character. Include unit tests for precedence (2+3*4 = 14), "
            "right-associativity (2^3^2 = 512), unary minus, parentheses, and the divide-by-zero rejection."
        ),
        "checks": [
            {"type": "command_succeeds", "name": "imports", "command": "python3 -m pytest --collect-only -q"},
            {"type": "command_succeeds", "name": "help", "command": "python3 -m rdcalc --help"},
            {
                "type": "command_succeeds",
                "name": "golden-precedence",
                "command": "python3 -m rdcalc eval '2+3*4' | grep -qE '(^|[^0-9])14([^0-9]|$)'",
            },
            {
                "type": "command_succeeds",
                "name": "golden-rightassoc",
                "command": "python3 -m rdcalc eval '2^3^2' | grep -qE '(^|[^0-9])512([^0-9]|$)'",
            },
            {"type": "command_succeeds", "name": "tests", "command": "python3 -m pytest -q"},
        ],
    },
    {
        "id": "txn",
        "bucket": "transaction",
        "app_slug": "txkvbench",
        "language": "python",
        "prompt": (
            "Build a command-line transactional key-value store in Python, package name txkvbench, run as "
            "python -m txkvbench. Persist to SQLite via a GLOBAL --db PATH option that comes BEFORE the "
            "subcommand. Subcommand: exec SCRIPT where SCRIPT is a single positional string of "
            "semicolon-separated commands, each one of: SET key value; GET key (print the value); DEL key; "
            "COUNT (print the number of keys); BEGIN; COMMIT; ROLLBACK. Support NESTED transactions (a "
            "BEGIN inside a BEGIN) using savepoints: a ROLLBACK undoes only the innermost transaction's "
            "changes; a COMMIT merges them into the enclosing scope; changes become durable only when the "
            "outermost transaction commits, or immediately when run outside any transaction. Validate with "
            "a clear error and a NONZERO exit code for EACH of: a GET of an unset key; a ROLLBACK or COMMIT "
            "with no open transaction; and an unknown command. Include unit tests for a single-level "
            "rollback undoing a SET, a nested rollback (inner discarded but outer kept), COMMIT "
            "persistence, and the get-unset rejection."
        ),
        "checks": [
            {"type": "command_succeeds", "name": "imports", "command": "python3 -m pytest --collect-only -q"},
            {"type": "command_succeeds", "name": "help", "command": "python3 -m txkvbench --help"},
            {
                "type": "command_succeeds",
                "name": "golden-nested-rollback",
                "command": (
                    "python3 -m txkvbench --db bench.db exec "
                    "'SET x 1; BEGIN; SET x 2; GET x; ROLLBACK; GET x; COUNT' "
                    "| tr '\\n' ' ' | grep -qE '2.*1.*1'"
                ),
            },
            {"type": "command_succeeds", "name": "tests", "command": "python3 -m pytest -q"},
        ],
    },
]

TIERS: Dict[str, Dict[str, Any]] = {
    "smoke": {"reps": 1, "specs": 1},
    "light": {"reps": 1, "specs": 3},
    "medium": {"reps": 5, "specs": 3},
    "high": {"reps": 10, "specs": 3},
    "extreme": {"reps": 10, "specs": 3, "hard": True},
}


def suite_for_tier(tier: str) -> List[int]:
    """Interleaved spec indices: spec0, spec1, spec2, spec0, ... so a partial run stays balanced."""
    spec = TIERS[tier]
    n = min(spec["specs"], len(SUITE))
    order: List[int] = []
    for _ in range(spec["reps"]):
        order.extend(range(n))
    return order


def run_suite(cfg, root, tier, variant) -> List[Dict[str, Any]]:
    apps_dir = (root / cfg["paths"]["apps"]).resolve()
    binary = (root / cfg["swarm"]["binary"]).resolve()
    endpoint = cfg["swarm"]["endpoint"]
    led = Ledger((root / cfg["paths"]["ledger"]).resolve())
    order = suite_for_tier(tier)
    if TIERS[tier].get("hard"):
        print("[bench] NOTE: tier 'extreme' harder-spec pressure is not yet differentiated; running like 'high'.")
    print(f"[bench] {variant}/{tier}: {len(order)} runs over {len(set(order))} fixed specs "
          f"(cap {_RUN_CAP_SECS}s/run)")
    out: List[Dict[str, Any]] = []
    for i, si in enumerate(order):
        spec = SUITE[si]
        slug = f"{spec['app_slug']}-{variant}-{i:02d}"
        ws = apps_dir / slug
        ws.mkdir(parents=True, exist_ok=True)
        task = TaskSpec(
            task_id=slug,
            archetype=spec["bucket"],
            app_slug=slug,
            workspace=str(ws),
            prompt=spec["prompt"],
            language=spec.get("language", "python"),
            deterministic_checks=[DeterministicCheck(**c) for c in spec["checks"]],
            seed=1000 + i,
        )
        t0 = time.monotonic()
        try:
            rr = run_swarm(binary, task, endpoint, [], _RUN_CAP_SECS)
            ev = collect(task, rr)
            v = verifier_mod.verify(ev, None)  # deterministic + cluster + diagnostics, NO brain
            vd = v.model_dump()
            wall = round(time.monotonic() - t0, 1)
            dims = {
                k: (dd.get("status") if isinstance(dd, dict) else dd)
                for k, dd in (vd.get("dimensions") or {}).items()
            }
            rec = {
                "session_id": slug,
                "ts": dt.datetime.now().isoformat(),
                "archetype": spec["bucket"],
                "app_slug": slug,
                "variant": variant,
                "tier": tier,
                "wall_secs": wall,
                "overall": vd.get("overall", "fail"),
                "tasks_done": len(rr.done or []),
                "tasks_failed": len(rr.failed or []),
                "exit_code": getattr(rr, "exit_code", None),
                "dims": dims,
                "run_dir": str(ws),
            }
            led.record(rec)
            out.append(rec)
            print(f"[bench {i + 1}/{len(order)}] {variant} {spec['id']} -> {rec['overall']} "
                  f"({wall}s · checks={dims.get('checks')} · done {rec['tasks_done']}/"
                  f"{rec['tasks_done'] + rec['tasks_failed']})")
        except Exception as e:  # a bad run must never kill the campaign
            print(f"[bench {i + 1}/{len(order)}] {spec['id']} run FAILED: {e}")
    return out


# --------------------------------------------------------------------------------------
# reporting
# --------------------------------------------------------------------------------------

def _rate(hits: int, n: int) -> str:
    return f"{(100.0 * hits / n):.0f}%" if n else "—"


def _p90(xs: List[float]) -> float:
    xs = sorted(xs)
    return xs[max(0, int(round(0.9 * (len(xs) - 1))))] if xs else 0.0


def _speed(xs: List[float]) -> Dict[str, float]:
    if not xs:
        return {"mean": 0.0, "median": 0.0, "p90": 0.0, "min": 0.0, "max": 0.0, "stdev": 0.0}
    return {
        "mean": round(stats.mean(xs), 1),
        "median": round(stats.median(xs), 1),
        "p90": round(_p90(xs), 1),
        "min": round(min(xs), 1),
        "max": round(max(xs), 1),
        "stdev": round(stats.pstdev(xs), 1),
    }


def _agg(sessions: List[Dict[str, Any]]) -> Dict[str, Any]:
    n = len(sessions)
    walls = [float(s["wall_secs"]) for s in sessions if s.get("wall_secs")]
    wins = sum(1 for s in sessions if s.get("overall") == "pass")
    partials = sum(1 for s in sessions if s.get("overall") == "partial")
    checks_pass = sum(1 for s in sessions if (s.get("dims") or {}).get("checks") == "pass")
    done = sum(s.get("tasks_done", 0) for s in sessions)
    total = done + sum(s.get("tasks_failed", 0) for s in sessions)
    return {
        "n": n,
        "wins": wins,
        "partials": partials,
        "fails": n - wins - partials,
        "usable_rate": _rate(wins + partials, n),
        "clean_rate": _rate(wins, n),
        "checks_rate": _rate(checks_pass, n),
        "task_ok_rate": _rate(done, total) if total else "—",
        "speed": _speed(walls),
    }


def bench_report(root, cfg, variant_filter: Optional[str] = None) -> str:
    led = Ledger((root / cfg["paths"]["ledger"]).resolve())
    sessions = [s for s in led.all_sessions() if s.get("variant")]
    if variant_filter:
        sessions = [s for s in sessions if s.get("variant") == variant_filter]
    if not sessions:
        return "No benchmark sessions recorded yet (run `bench --tier <t> --variant <v>`)."

    variants = sorted({s["variant"] for s in sessions})
    buckets = [b["bucket"] for b in SUITE]
    seen = {s.get("archetype") for s in sessions}
    buckets = [b for b in buckets if b in seen] + [b for b in sorted(seen) if b not in buckets]

    L: List[str] = ["# swarm-gym GGUF-vs-MLX benchmark\n",
                    f"{len(sessions)} runs · variants: {', '.join(variants)}\n"]

    for v in variants:
        vs = [s for s in sessions if s["variant"] == v]
        a = _agg(vs)
        sp = a["speed"]
        L.append(f"\n## {v}  ({a['n']} runs)\n")
        L.append(f"- outcomes: {a['wins']} clean / {a['partials']} partial / {a['fails']} fail "
                 f"(usable {a['usable_rate']}, clean {a['clean_rate']})")
        L.append(f"- judged-by-running (golden checks pass): {a['checks_rate']} · swarm task success: {a['task_ok_rate']}")
        L.append(f"- wall-clock/run (s): median {sp['median']} · mean {sp['mean']} · p90 {sp['p90']} "
                 f"· min {sp['min']} · max {sp['max']} · stdev {sp['stdev']}")
        L.append("\n| spec | n | usable | clean | checks | median s | p90 s | stdev |")
        L.append("| --- | --- | --- | --- | --- | --- | --- | --- |")
        for b in buckets:
            g = [s for s in vs if s.get("archetype") == b]
            if not g:
                continue
            ga = _agg(g)
            L.append(f"| {b} | {ga['n']} | {ga['usable_rate']} | {ga['clean_rate']} | "
                     f"{ga['checks_rate']} | {ga['speed']['median']} | {ga['speed']['p90']} | {ga['speed']['stdev']} |")

    if len(variants) >= 2:
        a, b = variants[0], variants[1]
        L.append(f"\n## head-to-head: {a} vs {b}\n")
        L.append(f"| spec | {a} median s | {b} median s | speedup | {a} checks | {b} checks |")
        L.append("| --- | --- | --- | --- | --- | --- |")
        for spec in ["overall"] + buckets:
            ga = _agg([s for s in sessions if s["variant"] == a and (spec == "overall" or s.get("archetype") == spec)])
            gb = _agg([s for s in sessions if s["variant"] == b and (spec == "overall" or s.get("archetype") == spec)])
            if not ga["n"] or not gb["n"]:
                continue
            ma, mb = ga["speed"]["median"], gb["speed"]["median"]
            speedup = f"{(ma / mb):.2f}×" if mb else "—"
            L.append(f"| {spec} | {ma} | {mb} | {speedup} | {ga['checks_rate']} | {gb['checks_rate']} |")
        L.append(f"\n_speedup = {a} median ÷ {b} median (>1 means {b} is the faster runtime)._")
    else:
        L.append(f"\n_Only variant '{variants[0]}' recorded — run the other variant for the head-to-head._")

    report_md = "\n".join(L) + "\n"
    out_path = (root / cfg["paths"]["runs"]).resolve() / "BENCHMARK.md"
    try:
        out_path.write_text(report_md)
        report_md += f"\n(written to {out_path})\n"
    except Exception:
        pass
    try:
        report_md += bench_csv(root, cfg) + "\n"
    except Exception as e:
        report_md += f"(csv export skipped: {e})\n"
    return report_md


# Columns are the deterministic, website-shareable fields — one row per run, plus a
# per-(variant, spec) summary. Regenerated from the ledger, so MLX + GGUF land in one file.
_RUN_COLS = [
    "variant", "tier", "spec", "app_slug", "session_id", "ts",
    "wall_secs", "functional", "overall", "checks", "swarm", "cluster", "diagnostics",
    "tasks_done", "tasks_failed", "exit_code",
]
_SUMMARY_COLS = [
    "variant", "spec", "n", "checks_pass_pct", "usable_pct",
    "wall_median_s", "wall_mean_s", "wall_p90_s", "wall_stdev_s", "wall_min_s", "wall_max_s",
]


def bench_csv(root, cfg) -> str:
    """Write benchmark-runs.csv (one row per run) + benchmark-summary.csv (per variant×spec).
    Deterministic fields only; safe to regenerate — every variant accumulates in the same files."""
    led = Ledger((root / cfg["paths"]["ledger"]).resolve())
    sessions = [s for s in led.all_sessions() if s.get("variant")]
    runs_dir = (root / cfg["paths"]["runs"]).resolve()
    if not sessions:
        return "csv: no benchmark sessions to export yet."

    runs_path = runs_dir / "benchmark-runs.csv"
    with open(runs_path, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(_RUN_COLS)
        for s in sorted(sessions, key=lambda x: (x.get("variant", ""), x.get("ts", ""))):
            d = s.get("dims") or {}
            checks = d.get("checks")
            w.writerow([
                s.get("variant"), s.get("tier"), s.get("archetype"), s.get("app_slug"),
                s.get("session_id"), s.get("ts"), s.get("wall_secs"),
                1 if checks == "pass" else 0, s.get("overall"),
                checks, d.get("swarm"), d.get("cluster"), d.get("diagnostics"),
                s.get("tasks_done"), s.get("tasks_failed"), s.get("exit_code"),
            ])

    groups: Dict[tuple, List[Dict[str, Any]]] = {}
    for s in sessions:
        groups.setdefault((s.get("variant"), s.get("archetype")), []).append(s)
    for v in sorted({s.get("variant") for s in sessions}):
        groups[(v, "ALL")] = [s for s in sessions if s.get("variant") == v]

    summ_path = runs_dir / "benchmark-summary.csv"
    with open(summ_path, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(_SUMMARY_COLS)
        for (v, spec), g in sorted(groups.items(), key=lambda kv: (kv[0][0] or "", kv[0][1] or "")):
            sp = _speed([float(x["wall_secs"]) for x in g if x.get("wall_secs")])
            checks_pct = round(100.0 * sum(1 for x in g if (x.get("dims") or {}).get("checks") == "pass") / len(g), 1)
            usable_pct = round(100.0 * sum(1 for x in g if x.get("overall") in ("pass", "partial")) / len(g), 1)
            w.writerow([v, spec, len(g), checks_pct, usable_pct,
                        sp["median"], sp["mean"], sp["p90"], sp["stdev"], sp["min"], sp["max"]])

    return f"csv: wrote {runs_path.name} ({len(sessions)} runs) + {summ_path.name} to {runs_dir}"
