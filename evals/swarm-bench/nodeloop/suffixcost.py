#!/usr/bin/env python3
"""What the repair suffix COSTS the fleet, in node-seconds.

F522 measured the suffix at 69% of window variance and suffix<->wall at +0.959: the repair loop is
the single biggest lever on how long a run takes. F517 then baked `spec_repair` ON, and on
2026-08-08 it fired for the first time in this bench's history -- three twins racing one finding set
across all three nodes.

That creates the exact blind spot F525/L331 named: racing N attempts can SHORTEN the suffix while
burning N times the node-seconds, and **every instrument I owned would have called that a pure win**,
because all of them measure the suffix in WALL-CLOCK. A lever that buys 3 minutes of wall for 20
node-minutes of fleet is not obviously worth having, and I could not even state the trade.

The reason the suffix was unmeasurable is structural, not an oversight: `occupancy.py` derives busy
time from `task_dispatched` -> `task_completed` intervals, and **the repair loop does not emit
those**. It emits `complete_fix_dispatched` with no matching completion event (keys confirmed by
dump, L264: round/twin/model/task_id/baseline_findings and nothing else). So the event channel is
blind there by construction, and no amount of care with it will help.

`fleetsample.sh` is an INDEPENDENT channel -- it polls `lms ps` every 30s and records what each node
is actually doing, whether or not the engine says anything. This instrument reads that channel.

Two things it must get right, both of which nearly produced a false finding today:

1. **BUSY IS NOT `GENERATING`.** During the race `lms ps` showed gabee=PROCESSINGPROMPT while the
   other two generated. A `grep -c GENERATING` says one node is working; the truth is three.
   PROCESSINGPROMPT is prompt ingestion -- real work, on a real node, blocking a real slot. Counting
   only GENERATING would have reported "the race collapsed to one node", which is the precise shape
   of false finding L332 exists to prevent. BUSY = GENERATING or PROCESSINGPROMPT.

2. **NO SAMPLES IS NOT ZERO BUSY.** An unsampled window must return None and be reported UNMEASURED,
   never an occupancy of 0.0. `all([])` is True and a mean of nothing is not a zero; a sampler that
   died would otherwise read as a perfectly idle fleet, i.e. as the strongest possible finding.

The sampler is fleet-GLOBAL, so a concurrent run's work lands in these numbers too. Every figure
here is "what the fleet was doing", not "what this run caused". With one run live they coincide;
`--check-exclusive` is how you find out whether they did.
"""
import json
import os
import sys
import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import occupancy  # noqa: E402

BUSY_STATES = ("GENERATING", "PROCESSINGPROMPT")
IDLE_STATES = ("IDLE",)

# A sample stands for the interval until the next one. If the sampler stalls (machine asleep, `lms`
# hanging), that gap is NOT time we observed -- crediting a 40-minute gap to whatever the last sample
# happened to show would invent the bulk of a window out of one reading.
MAX_SAMPLE_SPAN = 90.0
SAMPLE_PERIOD = 30.0

# Below this fraction of a window actually observed, the numbers are reported but not to be believed.
MIN_COVERAGE = 0.6

# COVERAGE IS NOT RESOLUTION, and conflating them published a number built from two readings. A
# 48-second suffix is fully covered -- coverage 1.00, `reliable` true -- while resting on 2 samples,
# where one node flipping state moves occupancy by 0.5. Two archived cells with sub-minute suffixes
# reported 0.454 and 0.908 on that basis, a spread that is pure quantisation and was about to be read
# as a difference between runs. A window must be observed AND sampled often enough to mean anything.
MIN_SAMPLES = 6


def _parse_detail(detail: str) -> dict:
    """`gabee=GENERATING mihai=IDLE qwen3=PROCESSINGPROMPT` -> {ident: state}.

    The sampler's awk does not reliably skip `lms ps`'s header, so a literal `IDENT=STATUS` pair
    rides along in the detail column. It is not a node and must never be counted as one.
    """
    out = {}
    for tok in (detail or "").split():
        if "=" not in tok:
            continue
        ident, _, state = tok.partition("=")
        if ident == "IDENT" or state == "STATUS":
            continue
        out[ident] = state
    return out


def read_samples(path) -> list[dict]:
    """Parse fleet-samples.tsv into epoch-stamped rows.

    The sampler writes `%H:%M:%S` with NO DATE, and the file spans many days. Dates are reconstructed
    by anchoring the LAST row to the file's mtime and walking BACKWARDS, decrementing the day
    whenever time steps forward (i.e. a midnight boundary read in reverse). Anchoring at the end
    rather than the start means the anchor is a fact (mtime) rather than a guess, and the rows that
    matter -- the recent ones -- are the ones closest to it.
    """
    p = Path(path)
    if not p.exists():
        return []
    rows = []
    for line in p.read_text(errors="replace").splitlines():
        if not line.strip() or line.startswith("ts\t"):
            continue
        parts = line.split("\t")
        if len(parts) < 4:
            continue
        try:
            hh, mm, ss = (int(x) for x in parts[0].split(":"))
        except ValueError:
            continue
        rows.append({"hms": (hh, mm, ss), "detail": parts[4] if len(parts) > 4 else ""})
    if not rows:
        return []

    anchor = datetime.datetime.fromtimestamp(p.stat().st_mtime)
    day = anchor.date()
    prev_hms = None
    for r in reversed(rows):
        if prev_hms is not None and r["hms"] > prev_hms:
            day -= datetime.timedelta(days=1)
        prev_hms = r["hms"]
        hh, mm, ss = r["hms"]
        r["t"] = datetime.datetime.combine(
            day, datetime.time(hh, mm, ss)).timestamp()

    for r in rows:
        states = _parse_detail(r["detail"])
        r["nodes"] = states
        r["busy"] = sum(1 for s in states.values() if s in BUSY_STATES)
        r["idle"] = sum(1 for s in states.values() if s in IDLE_STATES)
    return rows


def window_busy(samples: list[dict], t_start: float, t_end: float) -> dict | None:
    """Busy node-seconds over [t_start, t_end). None when the window was never sampled.

    Each sample covers until the next one, clipped to the window and capped at MAX_SAMPLE_SPAN, so a
    sampler outage subtracts from `coverage` instead of being silently credited or silently zeroed.
    """
    if t_end <= t_start or not samples:
        return None
    wall = t_end - t_start
    busy_secs = 0.0
    observed = 0.0
    per_node: dict[str, float] = {}
    peak = 0
    n_samples = 0
    for i, s in enumerate(samples):
        nxt = samples[i + 1]["t"] if i + 1 < len(samples) else s["t"] + SAMPLE_PERIOD
        span_end = min(nxt, s["t"] + MAX_SAMPLE_SPAN)
        lo, hi = max(s["t"], t_start), min(span_end, t_end)
        if hi <= lo:
            continue
        dur = hi - lo
        observed += dur
        n_samples += 1
        busy_secs += s["busy"] * dur
        peak = max(peak, s["busy"])
        for ident, state in s["nodes"].items():
            if state in BUSY_STATES:
                per_node[ident] = per_node.get(ident, 0.0) + dur
    if observed <= 0:
        return None

    coverage = observed / wall
    # Occupancy is over the OBSERVED time, not the whole window: dividing measured busy-seconds by
    # unmeasured wall would report a sampler outage as idleness.
    #
    # The denominator is DISTINCT IDENTIFIERS, never the pool's slot count. `lms ps` prints one
    # status per identifier, so a node running both its slots still reads as one busy node -- the
    # numerator can never exceed the node count. Dividing by slots (6 = 3 nodes x weight 2) mixed a
    # node-count numerator with a slot denominator and halved every figure this file produced: a
    # fully saturated 3-node fleet reported occ 0.437 instead of ~0.9.
    denom_nodes = max((len(s["nodes"]) for s in samples), default=0) or 1
    return {
        "wall_secs": round(wall, 1),
        "observed_secs": round(observed, 1),
        "coverage": round(coverage, 3),
        "samples": n_samples,
        "reliable": coverage >= MIN_COVERAGE and n_samples >= MIN_SAMPLES,
        "busy_node_secs": round(busy_secs, 1),
        "occupancy": round(busy_secs / (observed * denom_nodes), 4),
        "peak_busy_nodes": peak,
        "nodes_seen": denom_nodes,
        "per_node_secs": {k: round(v, 1) for k, v in sorted(per_node.items())},
    }


def analyse(run_dir, samples_path=None) -> dict:
    """Per-phase fleet cost for one run, plus the repair race if one happened."""
    run_dir = Path(run_dir)
    log = run_dir / "run.jsonl"
    if not log.exists():
        return {"error": f"no run log at {log}"}
    events = occupancy.read_events(log)
    if not events:
        return {"error": "empty event stream"}

    samples_path = samples_path or (run_dir.parent / "fleet-samples.tsv")
    samples = read_samples(samples_path)

    def t_of(ev):
        return occupancy.parse_ts(ev.get("ts"))

    started = next((t_of(e) for e in events if e.get("event") == "run_started"), None)
    if started is None:
        return {"error": "no run_started"}
    slots = None
    for e in events:
        if e.get("event") == "run_started":
            slots = occupancy.slot_count(e.get("pool") or [])
            break

    first_dispatch = next((t_of(e) for e in events if e.get("event") == "task_dispatched"), None)
    completions = [t_of(e) for e in events if e.get("event") == "task_completed"]
    last_completion = max(completions) if completions else None
    finished = next((t_of(e) for e in reversed(events) if e.get("event") == "run_finished"), None)
    live = finished is None
    end = finished if finished is not None else datetime.datetime.now().timestamp()

    fix_dispatches = [e for e in events if e.get("event") == "complete_fix_dispatched"]

    out = {
        "run_dir": str(run_dir),
        "live": live,
        "slots": slots,
        "samples_file": str(samples_path),
        "samples_total": len(samples),
        "phases": {},
        "race": None,
    }

    bounds = [("prefix", started, first_dispatch),
              ("execute", first_dispatch, last_completion),
              ("suffix", last_completion, end)]
    for name, a, b in bounds:
        out["phases"][name] = (window_busy(samples, a, b)
                               if a is not None and b is not None else None)

    if fix_dispatches:
        race_start = min(t_of(e) for e in fix_dispatches)
        by_round: dict = {}
        for e in fix_dispatches:
            by_round.setdefault(e.get("round"), []).append(e)
        twins = max(len(v) for v in by_round.values())
        w = window_busy(samples, race_start, end)
        out["race"] = {
            "rounds": len(by_round),
            "twins_max": twins,
            "models": sorted({str(e.get("model", ""))[:28] for e in fix_dispatches}),
            "baseline_findings": fix_dispatches[0].get("baseline_findings"),
            "window": w,
            # The counterfactual the lever has to beat: one node, same wall-clock. Racing is only
            # worth its price if it ENDS the suffix sooner, and this is the multiple it must repay.
            "cost_multiple_vs_serial": (round(w["busy_node_secs"] / w["observed_secs"], 2)
                                        if w and w["observed_secs"] > 0 else None),
        }

    ev_based = occupancy.analyse(str(log))
    out["cross_check"] = {
        "event_execute_occupancy": ev_based.get("occupancy"),
        "sampler_execute_occupancy": (out["phases"]["execute"] or {}).get("occupancy"),
    }
    return out


def render(a: dict) -> str:
    if "error" in a:
        return f"suffixcost: {a['error']}"
    L = [f"FLEET COST  {Path(a['run_dir']).name}"
         f"{'  [LIVE]' if a['live'] else ''}   slots={a['slots']}  samples={a['samples_total']}"]
    if not a["samples_total"]:
        L.append("  NO FLEET SAMPLES — every figure below is UNMEASURED, not zero.")
        return "\n".join(L)

    for name in ("prefix", "execute", "suffix"):
        w = a["phases"].get(name)
        if not w:
            L.append(f"  {name:<8} UNMEASURED (no samples in window)")
            continue
        flag = ("" if w["reliable"]
                else f"  ⚠ UNRELIABLE ({w['coverage']*100:.0f}% observed, {w['samples']} samples)")
        L.append(f"  {name:<8} wall {w['wall_secs']/60:6.1f}m   busy {w['busy_node_secs']/60:7.1f} node-min"
                 f"   occ {w['occupancy']:.3f}   peak {w['peak_busy_nodes']}/{w['nodes_seen']}{flag}")

    r = a.get("race")
    if r:
        L.append(f"  RACE   rounds={r['rounds']} twins={r['twins_max']} "
                 f"baseline_findings={r['baseline_findings']}")
        w = r["window"]
        if w:
            L.append(f"         {w['busy_node_secs']/60:.1f} node-min over {w['wall_secs']/60:.1f}m wall"
                     f"  ⇒ {r['cost_multiple_vs_serial']}x the fleet a serial fix would use")
            for ident, secs in w["per_node_secs"].items():
                L.append(f"           {ident:<8} {secs/60:6.1f} node-min busy")
        else:
            L.append("         window UNMEASURED")
    else:
        L.append("  RACE   none this run (no complete_fix_dispatched)")

    c = a["cross_check"]
    ev, sm = c["event_execute_occupancy"], c["sampler_execute_occupancy"]
    if ev is not None and sm is not None:
        # Not an equality check -- the two channels measure different quantities, and the invariant
        # between them is what makes the sampler trustworthy where events are blind. Events report
        # SLOT occupancy (6 concurrent tasks possible); the sampler reports NODE utilisation. Any
        # busy slot makes its node busy, so node utilisation is >= slot occupancy, always. An
        # arbitrary "within 0.15" tolerance passed here for the wrong reason while the sampler figure
        # was itself deflated by a wrong denominator.
        ok = sm + 1e-9 >= ev
        verdict = "consistent" if ok else "IMPOSSIBLE — sampler below slot occupancy, channel is broken"
        L.append(f"  control  slot occ (events) {ev:.3f} <= node occ (sampler) {sm:.3f} → {verdict}")
    else:
        L.append("  control  execute occupancy uncomparable (one channel silent)")
    return "\n".join(L)


def _s(t, detail):
    hh = int(t // 3600) % 24
    mm = int(t // 60) % 60
    ss = int(t) % 60
    return f"{hh:02d}:{mm:02d}:{ss:02d}\t0\t0\t0\t{detail}"


def self_test() -> int:
    """Controls in BOTH directions, plus the two traps this file exists to avoid."""
    import tempfile
    fails = []

    def mk(lines):
        f = tempfile.NamedTemporaryFile("w", suffix=".tsv", delete=False)
        f.write("ts\tgenerating\tprocessing\tidle\tdetail\n")
        f.write("\n".join(lines) + "\n")
        f.close()
        return f.name

    base = 12 * 3600
    # NEGATIVE control: an idle fleet must score 0.0 occupancy.
    idle = read_samples(mk([_s(base + i * 30, "a=IDLE b=IDLE c=IDLE") for i in range(20)]))
    w = window_busy(idle, idle[0]["t"], idle[-1]["t"] + 30)
    if not w or w["occupancy"] != 0.0:
        fails.append(f"idle fleet scored {w and w['occupancy']}, must be 0.0")

    # POSITIVE control: a saturated fleet must score 1.0.
    busy = read_samples(mk([_s(base + i * 30, "a=GENERATING b=GENERATING c=GENERATING")
                            for i in range(20)]))
    w = window_busy(busy, busy[0]["t"], busy[-1]["t"] + 30)
    if not w or w["occupancy"] < 0.99:
        fails.append(f"saturated fleet scored {w and w['occupancy']}, must be ~1.0")

    # THE FINDING THAT NEARLY SHIPPED: PROCESSINGPROMPT is busy. A fleet in prompt ingestion is a
    # fleet at work; scoring it idle reports a three-node race as a one-node collapse.
    proc = read_samples(mk([_s(base + i * 30, "a=PROCESSINGPROMPT b=PROCESSINGPROMPT c=GENERATING")
                            for i in range(20)]))
    w = window_busy(proc, proc[0]["t"], proc[-1]["t"] + 30)
    if not w or w["occupancy"] < 0.99:
        fails.append(f"PROCESSINGPROMPT scored {w and w['occupancy']}, must count as busy")
    if w and w["peak_busy_nodes"] != 3:
        fails.append(f"peak busy {w and w['peak_busy_nodes']}, must be 3")

    # VACUOUS TRUTH: an unsampled window is None, never a confident zero.
    far = busy[-1]["t"] + 86400
    if window_busy(busy, far, far + 600) is not None:
        fails.append("unsampled window returned a number instead of None")
    if window_busy([], far, far + 600) is not None:
        fails.append("empty sampler returned a number instead of None")

    # The `lms ps` header must never be counted as a node.
    hdr = read_samples(mk([_s(base + i * 30, "IDENT=STATUS a=GENERATING") for i in range(4)]))
    if hdr[0]["nodes"] != {"a": "GENERATING"}:
        fails.append(f"header leaked into node map: {hdr[0]['nodes']}")

    # A sampler outage must cut coverage, not be credited to the last reading seen.
    gap = read_samples(mk([_s(base, "a=GENERATING"), _s(base + 600, "a=GENERATING")]))
    w = window_busy(gap, gap[0]["t"], gap[-1]["t"] + 30)
    if not w or w["reliable"]:
        fails.append(f"a 10-minute outage reported reliable (coverage {w and w['coverage']})")
    if w and w["busy_node_secs"] > 2 * MAX_SAMPLE_SPAN:
        fails.append(f"outage credited {w['busy_node_secs']}s of busy from 2 samples")

    # Midnight rollover: times stepping backwards across the file are earlier days, not negative gaps.
    roll = read_samples(mk([_s(23 * 3600 + 59 * 60, "a=IDLE"), _s(1, "a=IDLE")]))
    if len(roll) == 2 and not (0 < roll[1]["t"] - roll[0]["t"] < 3600):
        fails.append(f"midnight rollover gave a {roll[1]['t'] - roll[0]['t']}s gap")

    # RESOLUTION IS NOT COVERAGE. A 90-second window is fully observed and still rests on 3 readings;
    # it must be UNRELIABLE despite coverage 1.0, or two-sample noise gets published as a difference
    # between runs. The long window beside it, identically covered, must stay reliable.
    short = window_busy(busy, busy[0]["t"], busy[0]["t"] + 90)
    if not short or short["reliable"]:
        fails.append(f"a 3-sample window reported reliable (coverage {short and short['coverage']})")
    long_w = window_busy(busy, busy[0]["t"], busy[-1]["t"] + 30)
    if not long_w or not long_w["reliable"]:
        fails.append("a fully-sampled 10-minute window reported unreliable")

    # Determinism: two passes over the SAME file must agree exactly.
    same = mk([_s(base + i * 30, "a=GENERATING b=IDLE") for i in range(8)])
    if json.dumps(read_samples(same)) != json.dumps(read_samples(same)):
        fails.append("read_samples is not deterministic on identical input")

    for f in fails:
        print(f"  FAIL {f}")
    print(f"suffixcost self-test: {'PASS' if not fails else str(len(fails)) + ' FAILURES'}")
    return 1 if fails else 0


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        sys.exit(self_test())
    target = next((a for a in sys.argv[1:] if not a.startswith("-")), None)
    if not target:
        base = Path(__file__).resolve().parent.parent / "runs" / "nodeloop"
        cands = [d for d in base.glob("*/run.jsonl")]
        if not cands:
            print("suffixcost: no run to analyse")
            sys.exit(0)
        target = str(max(cands, key=lambda p: p.stat().st_mtime).parent)
    a = analyse(target)
    print(render(a))
    if "--json" in sys.argv:
        print(json.dumps(a, indent=2))
