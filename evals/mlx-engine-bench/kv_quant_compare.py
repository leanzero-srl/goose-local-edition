#!/usr/bin/env python3
"""Join kv_quant.py runs into the KV-cache compression table.

Usage: kv_quant_compare.py <results dir> --reference bf16 [--noise bf16-repeat] <candidate labels...>

QUALITY is greedy token agreement against the reference run of the same prompts: the tokens the
candidate emitted before its first divergence from the reference, over the reference's tokens.
After a divergence the two continue from different contexts, so later positions are not compared.
At each divergence the reference's own top-1 − top-2 logprob margin is reported: a small margin is
a near-tie the reference itself barely decided; a large one is a real change of mind. The noise
run (the reference configuration run again) is the floor every candidate is judged against.
"""

import argparse
import json
import os
import re
import statistics

GIB = 1024**3


def load(results_dir: str, label: str) -> dict:
    with open(os.path.join(results_dir, f"{label}.json")) as f:
        return json.load(f)


def token_ids(run: dict) -> list[str]:
    return [t["token"] for t in run["tokens"]]


def compare_prompt(ref: dict, cand: dict) -> dict:
    a, b = token_ids(ref), token_ids(cand)
    n = 0
    while n < len(a) and n < len(b) and a[n] == b[n]:
        n += 1
    same = n == len(a) == len(b)
    row = {"ref_tokens": len(a), "cand_tokens": len(b), "prefix": n, "identical": same}
    if not same and n < len(a):
        top = ref["tokens"][n]["top"]
        if len(top) >= 2 and top[0][1] is not None and top[1][1] is not None:
            row["ref_margin"] = top[0][1] - top[1][1]
        row["ref_token"] = a[n]
        row["cand_token"] = b[n] if n < len(b) else None
        ranks = [t[0] for t in top]
        row["cand_rank_in_ref_top5"] = ranks.index(b[n]) + 1 if n < len(b) and b[n] in ranks else None
    tools_a = [(c["name"], c["arguments"]) for c in ref.get("tool_calls") or []]
    tools_b = [(c["name"], c["arguments"]) for c in cand.get("tool_calls") or []]
    row["tool_calls_equal"] = tools_a == tools_b
    row["tool_names"] = [c[0] for c in tools_b]
    return row


def final_number(run: dict) -> str | None:
    """The last number the arithmetic answer states (the right one is 30 jobs)."""
    numbers = re.findall(r"\d+", (run["quality"].get("arithmetic") or {}).get("content") or "")
    return numbers[-1] if numbers else None


def quality_table(ref: dict, cand: dict) -> tuple[list[str], dict]:
    lines = []
    matched = total = identical = 0
    margins = []
    for key, r in ref["quality"].items():
        c = cand["quality"].get(key)
        if c is None:
            lines.append(f"| {key} | missing in candidate | | | |")
            continue
        row = compare_prompt(r, c)
        matched += row["prefix"]
        total += row["ref_tokens"]
        identical += row["identical"]
        if "ref_margin" in row:
            margins.append(row["ref_margin"])
        div = (
            "identical"
            if row["identical"]
            else f"@{row['prefix']}: {row.get('ref_token')!r} → {row.get('cand_token')!r} "
            f"(margin {row.get('ref_margin', float('nan')):.3f}, rank {row.get('cand_rank_in_ref_top5')})"
        )
        extra = ""
        if key.startswith("retrieval"):
            extra = "found" if "KESTREL-4471" in (c.get("content") or "") else f"MISSED: {c.get('content')!r}"
        elif key.startswith("c-agent"):
            extra = f"tools {'=' if row['tool_calls_equal'] else '≠'} {row['tool_names']}"
        lines.append(f"| {key} | {row['prefix']}/{row['ref_tokens']} | {div} | {extra} |")
    summary = {
        "arithmetic": final_number(cand),
        "tools_equal": sum(
            compare_prompt(ref["quality"][k], cand["quality"][k])["tool_calls_equal"]
            for k in ref["quality"]
            if k.startswith("c-agent") and k in cand["quality"]
        ),
        "agreement": matched / total if total else None,
        "identical": identical,
        "prompts": len(ref["quality"]),
        "median_margin": statistics.median(margins) if margins else None,
        "max_margin": max(margins) if margins else None,
    }
    return lines, summary


def memory_rows(run: dict) -> list[dict]:
    rows = []
    for ctx, m in sorted(run.get("memory", {}).items(), key=lambda kv: int(kv[0])):
        rows.append(
            {
                "ctx": int(ctx),
                "prompt": m["prompt_tokens"],
                "ttft": m["ttft_s"],
                "prefill": m["prefill_tps"],
                "decode": m["decode_tps"],
                "decode_delta_gb": (m["active_decode_max_gb"] or 0) - m["active_before_gb"],
                "prefill_delta_gb": (m["active_prefill_max_gb"] or 0) - m["active_before_gb"],
                "peak_after": m["process_peak_after_gb"],
                "answer": m["content"],
            }
        )
    return rows


def slope_bytes_per_token(rows: list[dict]) -> float | None:
    if len(rows) < 2:
        return None
    lo, hi = rows[-2], rows[-1]
    return (hi["decode_delta_gb"] - lo["decode_delta_gb"]) * 1e9 / (hi["prompt"] - lo["prompt"])


def retrieval_found(run: dict) -> bool:
    r = run["quality"].get("retrieval-31k") or {}
    return "KESTREL-4471" in (r.get("content") or "")


def warm_decode(run: dict) -> tuple[float, int] | None:
    """Median decode tok/s of the `decode` phase's warm requests, and their prompt size."""
    warm = [x for x in (run.get("decode") or {}).get("runs", []) if not x["cold"] and x["decode_tps"]]
    if not warm:
        return None
    return statistics.median(x["decode_tps"] for x in warm), warm[0]["prompt_tokens"]


def mode_record(ref: dict, cand: dict, summary: dict, speed_ref: dict | None, speed_cand: dict | None) -> dict:
    out = {
        "agreement": round(summary["agreement"], 4),
        "identicalAnswers": summary["identical"],
        "retrievalFound": retrieval_found(cand),
    }
    ref_speed = warm_decode(speed_ref) if speed_ref else None
    cand_speed = warm_decode(speed_cand) if speed_cand else None
    if ref_speed and cand_speed:
        out["decodeTpsRatio"] = round(cand_speed[0] / ref_speed[0], 3)
        out["decodeContextTokens"] = cand_speed[1]
    return out


def write_record(model_dir: str, results_dir: str, engine: str, ref: dict, noise: str, summaries: dict, runs: dict,
                 modes: dict, speed: dict):
    """goose's per-model measurement record (goose_sidecar::kv_cache::MEASUREMENT_FILE).

    `modes` maps a cache mode to the label measured for it; `speed` maps "bf16"/a mode to the label
    whose `decode` phase carries its tok/s (speed runs keep MTP on, quality runs cannot)."""
    speed_runs = {k: load(results_dir, v) for k, v in speed.items()}
    record = {
        "measuredAt": os.path.basename(os.path.normpath(results_dir))[:10],
        "engine": engine,
        "prompts": summaries[noise]["prompts"],
        "noiseFloor": mode_record(ref, runs[noise], summaries[noise], None, None),
        "modes": {
            mode: mode_record(ref, runs[label], summaries[label], speed_runs.get("bf16"), speed_runs.get(mode))
            for mode, label in modes.items()
        },
        "source": os.path.relpath(results_dir, os.path.join(HERE, "..", "..")),
    }
    path = os.path.join(os.path.expanduser(model_dir), "goose-kv-cache.json")
    with open(path, "w") as f:
        json.dump(record, f, indent=1)
        f.write("\n")
    print(f"record: {path}\n{json.dumps(record, indent=1)}")


HERE = os.path.dirname(os.path.abspath(__file__))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("results_dir")
    ap.add_argument("--reference", default="bf16")
    ap.add_argument("--noise", default=None)
    ap.add_argument("--record", default=None, help="model directory to write goose-kv-cache.json into")
    ap.add_argument("--engine", default=None, help="engine version named in the record")
    ap.add_argument("--record-modes", default="int8=int8,int4=int4", help="mode=label pairs whose quality goes in the record")
    ap.add_argument("--record-speed", default="bf16=bf16,int8=int8,int4=int4", help="mode=label pairs carrying the decode phase")
    ap.add_argument("--summary", default="summary.md", help="file name for the markdown table in results_dir")
    ap.add_argument("candidates", nargs="+")
    args = ap.parse_args()
    if args.record and not (args.noise and args.engine):
        ap.error("--record needs --noise (the floor) and --engine")
    ref = load(args.results_dir, args.reference)
    labels = ([args.noise] if args.noise else []) + args.candidates
    md = [f"# KV-cache compression — reference `{args.reference}`", ""]
    summaries = {}
    for label in labels:
        cand = load(args.results_dir, label)
        lines, summary = quality_table(ref, cand)
        summaries[label] = summary
        md += [
            f"## quality: {label} vs {args.reference}",
            "",
            f"greedy token agreement {100 * summary['agreement']:.1f}% · identical answers "
            f"{summary['identical']}/{summary['prompts']} · median top-1 margin at divergence "
            f"{summary['median_margin']} (max {summary['max_margin']})",
            "",
            "| prompt | tokens before first divergence | first divergence (reference margin, candidate rank) | check |",
            "|---|---|---|---|",
            *lines,
            "",
        ]
    md += [
        "## quality summary",
        "",
        "| config vs reference | agreement to first divergence | identical answers | median / max reference margin at divergence (nats) | fact at 31k | arithmetic final (right: 30; reference said {}) | agent tool calls equal |".format(final_number(ref)),
        "|---|---|---|---|---|---|---|",
    ]
    for label in labels:
        sm = summaries[label]
        cand = load(args.results_dir, label)
        md.append(
            f"| {label} vs {args.reference} | {100 * sm['agreement']:.1f}% | {sm['identical']}/{sm['prompts']} | "
            f"{sm['median_margin']} / {sm['max_margin']} | {'found' if retrieval_found(cand) else 'MISSED'} | "
            f"{sm['arithmetic']} | {sm['tools_equal']}/4 |"
        )
    md += [
        "",
        "## memory and prefill (streaming, MTP on, no logprobs; one fresh engine per configuration)",
        "",
        "Peak = the engine's Metal peak after the request (process-wide, contexts run in ascending order).",
        "Prefix cache = the engine's retained entries after the request, within its fixed memory budget.",
        "",
        "| config | context | prompt tok | TTFT s | prefill tok/s | peak GB | Δ peak vs bf16 | prefix cache after (entries, GB) | 128k answer |",
        "|---|---|---|---|---|---|---|---|---|",
    ]
    ref_peaks = {r["ctx"]: r["peak_after"] for r in memory_rows(ref)}
    with_memory = [l for l in [args.reference] + labels if "memory" in load(args.results_dir, l)]
    for label in with_memory:
        run = load(args.results_dir, label)
        for r, (ctx, m) in zip(memory_rows(run), sorted(run["memory"].items(), key=lambda kv: int(kv[0]))):
            ans = r["answer"].replace("\n", " / ")[:60] if r["ctx"] >= 100000 else ""
            cache = m["cache_after"]
            delta = r["peak_after"] - ref_peaks.get(r["ctx"], r["peak_after"])
            md.append(
                f"| {label} | {r['ctx']} | {r['prompt']} | {r['ttft']:.0f} | {r['prefill']:.0f} | {r['peak_after']:.2f} | "
                f"{delta:+.2f} | {cache['entry_count']}, {cache['current_memory_mb'] / 1024:.1f} | {ans} |"
            )
    decode_rows = []
    for label in [args.reference] + labels:
        run = load(args.results_dir, label)
        if "decode" in run:
            warm = [x for x in run["decode"]["runs"] if not x["cold"]]
            tps = sorted(x["decode_tps"] for x in warm if x["decode_tps"])
            decode_rows.append(
                f"| {label} | {warm[0]['prompt_tokens']} | {', '.join(f'{t:.1f}' for t in tps)} | "
                f"{statistics.median(tps):.1f} | {', '.join(str(x['completion_tokens']) for x in warm)} |"
            )
    if decode_rows:
        md += [
            "",
            "## decode at ~32k context (MTP on, prefix cached, 300-token answers)",
            "",
            "| config | prompt tok | decode tok/s per request | median | completion tokens |",
            "|---|---|---|---|---|",
            *decode_rows,
        ]
    if args.record:
        runs = {label: load(args.results_dir, label) for label in labels}
        pairs = lambda text: dict(kv.split("=", 1) for kv in text.split(","))  # noqa: E731
        write_record(args.record, args.results_dir, args.engine, ref, args.noise, summaries, runs,
                     pairs(args.record_modes), pairs(args.record_speed))
    out = os.path.join(args.results_dir, args.summary)
    with open(out, "w") as f:
        f.write("\n".join(md) + "\n")
    print("\n".join(md))
    print(f"\nwritten: {out}")


if __name__ == "__main__":
    main()
