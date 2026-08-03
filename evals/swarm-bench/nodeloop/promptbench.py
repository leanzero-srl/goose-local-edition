#!/usr/bin/env python3
"""Replay REAL archived worker decision points against the fleet. Seconds per sample, not hours.

WHY THIS EXISTS (Lesson 78: if the feedback loop is hours, build the offline one first).
The mini-goal is "move the test-author failure row". Measuring it through a full swarm run costs
~85 minutes and yields ~5 test-author observations — and clearing p<0.05 against the old 13/42 rate
needs NINE clean ones, i.e. two or three runs, i.e. four hours per verdict. Twenty-one consecutive
ticks produced no metric movement for exactly this reason: the arm was not wrong, the sampling rate
was. `judge_replay.rs` already proved the shape works (100 minutes -> 0.00s) for the judge; this is
the same move for the WORKER, which is where the failure actually lives.

WHAT IT REPLAYS. `~/.local/state/goose/logs/llm_request.*.jsonl` holds one request row per file:
`input.{messages, tools, model, stream}` — the EXACT payload goose put on the wire, including the
full conversation up to that turn. So a case is not a synthetic prompt; it is the real state of a
real worker at a real decision point, tools and all.

WHAT IT MEASURES, AND WHAT THAT IS WORTH. One turn: does the model ACT (emit a tool call) or TALK
(emit text and stop)? The engine-side failure is "finished WITHOUT writing your owned file(s)" —
a worker completing its turn without acting. This bench sees that choice directly. It does NOT see
the whole task, so it is a PROXY: a variant that wins here has moved the per-turn action rate, which
is a necessary but not sufficient condition for moving the run-level row. Say that in any writeup.

THE CONTROL THAT MAKES IT MEANINGFUL. Select cases where the BASELINE reliably refuses to act
(`triage` does this by replaying each case n times at baseline). A variant is only interesting if it
moves those. A bench run only on cases the baseline already passes measures nothing — that is the
vacuous-truth trap in a new costume.

Usage:
    python3 promptbench.py harvest                       # build cases.jsonl from the archive
    python3 promptbench.py triage --n 5 [--limit 20]     # find cases the baseline FAILS
    python3 promptbench.py run --variant prefill --n 5   # replay one variant over the hard cases
    python3 promptbench.py report                        # the table
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import datetime
import glob
import json
import os
import pathlib
import re
import time
import urllib.request

import failures  # the metric's OWN kind classifier — Lesson 2

HERE = pathlib.Path(__file__).resolve().parent
BENCH = HERE / "bench"
CASES = BENCH / "cases.jsonl"
LOGS = pathlib.Path(os.path.expanduser("~/.local/state/goose/logs"))
ENDPOINT = "http://localhost:1234/v1/chat/completions"

WRITE_TOOLS = {"write", "edit", "developer__text_editor", "str_replace_editor"}
BUDGET_TOKENS = 1200          # shared by EVERY variant; see replay()'s docstring


class _Acted(Exception):
    """Raised to abandon the stream once the model has committed to a tool call."""


# ---------------------------------------------------------------- harvest

def request_of(path: str) -> dict | None:
    """The one request row in an llm_request file.

    F219: the newest file can be 0 bytes (request in flight) and most rows are `{data,usage}`
    streaming records, not requests. Only a row carrying `input.messages` is replayable.
    """
    try:
        with open(path) as fh:
            for line in fh:
                if not line.startswith("{"):
                    continue
                try:
                    r = json.loads(line)
                except Exception:
                    continue
                if ((r.get("input") or {}).get("messages")):
                    return r["input"]
    except OSError:
        return None
    return None


def system_text(inp: dict) -> str:
    m = inp["messages"][0]
    if m.get("role") != "system":
        return ""
    c = m.get("content")
    if isinstance(c, list):                       # content can be a list of {text:...} parts
        c = "".join(x.get("text", "") for x in c if isinstance(x, dict))
    return str(c or "")


WORKER_MARK = "You are a WORKER on a local AI swarm"


def msg_text(m: dict) -> str:
    c = m.get("content")
    if isinstance(c, list):
        c = "".join(x.get("text", "") for x in c if isinstance(x, dict))
    return str(c or "")


def owned_files(user_text: str) -> list[str]:
    """The files this worker was told it owns.

    ⚠ MY FIRST VERSION READ THE SYSTEM PROMPT AND MATCHED `- ` BULLETS. It found 2 test-authors in
    42 cases against a known 31% dispatch share, because the system prompt does not name the owned
    files at all AND because "SUBTASK" — my worker discriminator — also appears in the SUPERVISOR
    prompt, so most of what I classified as workers were judge calls. The canonical emitter is the
    dispatch's own user message: `**File owned:** \\`path\\``.
    """
    files: list[str] = []
    for chunk in re.findall(r"\*\*Files? owned:\*\*\s*(.+)", user_text):
        files += [p for p in re.findall(r"`([^`]+)`", chunk) if "." in p]
    if not files:                        # many dispatches name the file inline instead
        files = [p for p in re.findall(r"(?:You owe|Write)[^\n`]{0,30}`([\w./-]+\.\w+)`", user_text)]
    seen, out = set(), []
    for f in files:
        if f not in seen:
            seen.add(f)
            out.append(f)
    return out


def task_id_of(user_text: str) -> str:
    m = re.search(r"##\s*Subtask:\s*([^\s—\n]+)", user_text)
    return m.group(1) if m else ""


def kind_of(task_id: str, owned: list[str]) -> str:
    """Delegated to failures.py — the metric's OWN classifier. Never a second implementation."""
    return failures.kind_of(task_id, owned)


def harvest(limit: int) -> int:
    BENCH.mkdir(exist_ok=True)
    files = [f for f in glob.glob(str(LOGS / "llm_request.*.jsonl")) if os.path.getsize(f) > 0]
    files.sort(key=os.path.getmtime, reverse=True)
    rows = []
    for f in files[:limit]:
        inp = request_of(f)
        if not inp:
            continue
        sysp = system_text(inp)
        if WORKER_MARK not in sysp:
            continue                                   # judge/supervisor/architect — not a worker
        utext = "\n".join(msg_text(m) for m in inp["messages"][1:] if m.get("role") == "user")
        owned = owned_files(utext)
        tid = task_id_of(utext)
        rows.append({
            "id": os.path.basename(f).replace("llm_request.", "").replace(".jsonl", ""),
            "file": f,
            "mtime": os.path.getmtime(f),
            "kind": kind_of(tid, owned),
            "task_id": tid,
            "owned": owned,
            "n_msgs": len(inp["messages"]),
            "n_tools": len(inp.get("tools") or []),
            "sys_chars": len(sysp),
            "user_chars": len(utext),
            # the engine's own words for the defect this campaign is chasing, when present
            "was_stalled": "none of the files you own exists on disk yet" in utext,
            "last_role": inp["messages"][-1].get("role"),
        })
    with CASES.open("w") as fh:
        for r in rows:
            fh.write(json.dumps(r) + "\n")
    by = {}
    for r in rows:
        by[r["kind"]] = by.get(r["kind"], 0) + 1
    print(f"harvested {len(rows)} worker decision points -> {CASES}")
    for k, v in sorted(by.items(), key=lambda x: -x[1]):
        print(f"   {v:4d}  {k}")
    return 0


def load_cases(kind: str | None) -> list[dict]:
    if not CASES.is_file():
        return []
    out = []
    for line in CASES.read_text().splitlines():
        if line.strip():
            r = json.loads(line)
            if kind is None or r["kind"] == kind:
                out.append(r)
    return out


# ---------------------------------------------------------------- variants

def variant_payload(inp: dict, variant: str) -> dict:
    """Transform the archived payload. Each variant changes exactly ONE thing vs baseline."""
    p = json.loads(json.dumps(inp))                    # deep copy; never mutate the archive
    p["stream"] = True
    p.pop("stream_options", None)
    if variant == "baseline":
        pass
    elif variant == "prefill":
        p["messages"].append({"role": "assistant", "content": "<think>\n\n</think>\n\n"})
    elif variant == "samplers":
        p.update({"temperature": 0.6, "top_p": 0.95, "top_k": 20, "repetition_penalty": 1.0})
    elif variant == "toolchoice":
        p["tool_choice"] = "required"
    elif variant == "maxtok":
        p["max_tokens"] = 2048
    elif variant == "nudge":
        p["messages"].append({"role": "user", "content":
                              "Your next message must be a tool call, not prose. "
                              "If your owned file is not written yet, write it now."})
    elif variant == "nothink":
        p["chat_template_kwargs"] = {"enable_thinking": False}
    else:
        raise SystemExit(f"unknown variant {variant}")
    p.setdefault("max_tokens", BUDGET_TOKENS)         # `maxtok` sets its own; everyone else shares
    return p


VARIANTS = ["baseline", "prefill", "samplers", "toolchoice", "maxtok", "nudge", "nothink"]


# ---------------------------------------------------------------- replay

def replay(payload: dict, timeout: int) -> dict:
    """One streamed completion. Returns the OBSERVED behaviour, never a verdict.

    THE METRIC IS `acted WITHIN THE BUDGET`, and that is deliberate. A replay generates as many
    tokens as a real turn, so an uncapped bench is not cheap — the first smoke sample was still
    running at 120s. Every variant shares one `max_tokens` budget and the stream is ABORTED at the
    first tool-call name, which makes each sample bounded AND matches the defect's own wording:
    "none of the files you own exists on disk yet … you have emitted 6076 characters of reasoning
    instead". A model that has not acted inside the budget is the failure being measured, not a
    truncation artifact. Registered before any variant was compared.
    """
    body = json.dumps(payload).encode()
    req = urllib.request.Request(ENDPOINT, data=body,
                                 headers={"Content-Type": "application/json"})
    t0 = time.time()
    tool_names, text_chars, reason_chars = [], 0, 0
    ttfa = None
    finish = None
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            for raw in resp:
                line = raw.decode("utf-8", "replace").strip()
                if not line.startswith("data:"):
                    continue
                data = line[5:].strip()
                if data == "[DONE]":
                    break
                try:
                    ev = json.loads(data)
                except Exception:
                    continue
                for ch in ev.get("choices") or []:
                    d = ch.get("delta") or {}
                    if ch.get("finish_reason"):
                        finish = ch["finish_reason"]
                    if d.get("content"):
                        text_chars += len(d["content"])
                    if d.get("reasoning_content"):
                        reason_chars += len(d["reasoning_content"])
                    for tc in d.get("tool_calls") or []:
                        nm = (tc.get("function") or {}).get("name")
                        if nm:
                            tool_names.append(nm)
                            if ttfa is None:
                                ttfa = time.time() - t0
                                raise _Acted
    except _Acted:
        pass                                          # decision observed; no need to buy the rest
    except Exception as exc:
        return {"error": f"{type(exc).__name__}: {exc}"[:160], "elapsed": time.time() - t0}
    return {
        "acted": bool(tool_names),
        "first_tool": tool_names[0] if tool_names else None,
        "wrote": any(t in WRITE_TOOLS for t in tool_names),
        "n_tools_called": len(tool_names),
        "ttfa": ttfa,
        "text_chars": text_chars,
        "reason_chars": reason_chars,
        "finish": finish,
        "elapsed": time.time() - t0,
    }


def sample(case: dict, variant: str, rep: int, timeout: int) -> dict:
    inp = request_of(case["file"])
    if not inp:
        return {"case": case["id"], "variant": variant, "rep": rep, "error": "case unreadable"}
    out = replay(variant_payload(inp, variant), timeout)
    out.update({"case": case["id"], "variant": variant, "rep": rep, "kind": case["kind"],
                "ts": datetime.datetime.now().isoformat(timespec="seconds")})
    return out


def run(variant: str, kind: str, n: int, limit: int, workers: int, timeout: int,
        hard_only: bool) -> int:
    BENCH.mkdir(exist_ok=True)
    cases = load_cases(kind)
    if hard_only:
        hard = set(hard_cases())
        cases = [c for c in cases if c["id"] in hard]
    cases = cases[:limit]
    if not cases:
        print("no cases — run `harvest` (and `triage` if using --hard)")
        return 1
    jobs = [(c, r) for c in cases for r in range(n)]
    out = BENCH / f"{variant}.jsonl"
    print(f"{variant}: {len(cases)} cases x {n} reps = {len(jobs)} samples, {workers} concurrent")
    done = 0
    with out.open("a") as fh, cf.ThreadPoolExecutor(max_workers=workers) as ex:
        futs = [ex.submit(sample, c, variant, r, timeout) for c, r in jobs]
        for fut in cf.as_completed(futs):
            rec = fut.result()
            fh.write(json.dumps(rec) + "\n")
            fh.flush()
            done += 1
            mark = "!" if rec.get("error") else ("A" if rec.get("acted") else ".")
            print(mark, end="", flush=True)
    print(f"\n{done} samples -> {out}")
    return 0


# ---------------------------------------------------------------- triage / report

def load_samples(variant: str) -> list[dict]:
    p = BENCH / f"{variant}.jsonl"
    if not p.is_file():
        return []
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()]


def hard_cases() -> list[str]:
    """Cases where the BASELINE failed to act at least once — the defect, reproduced offline.

    A variant tested only where the baseline already acts cannot show anything.
    """
    by: dict[str, list[dict]] = {}
    for s in load_samples("baseline"):
        if not s.get("error"):
            by.setdefault(s["case"], []).append(s)
    return [c for c, ss in by.items() if any(not s.get("acted") for s in ss)]


def summarise(variant: str) -> dict | None:
    ss = [s for s in load_samples(variant) if not s.get("error")]
    errs = len(load_samples(variant)) - len(ss)
    if not ss:
        return None
    n = len(ss)
    acted = sum(1 for s in ss if s.get("acted"))
    wrote = sum(1 for s in ss if s.get("wrote"))
    ttfas = sorted(s["ttfa"] for s in ss if s.get("ttfa"))
    reasons = sorted(s.get("reason_chars", 0) for s in ss)
    return {
        "variant": variant, "n": n, "errors": errs,
        "acted": acted, "acted_pct": acted / n,
        "wrote": wrote, "wrote_pct": wrote / n,
        "ttfa_med": ttfas[len(ttfas) // 2] if ttfas else None,
        "reason_med": reasons[len(reasons) // 2],
        "cases": len({s["case"] for s in ss}),
    }


def report() -> int:
    hard = hard_cases()
    print(f"HARD CASES (baseline failed to act at least once): {len(hard)}")
    print(f"{'variant':<12} {'n':>4} {'cases':>6} {'err':>4} {'ACTED':>12} {'WROTE':>12} "
          f"{'ttfa_s':>7} {'think_c':>8}")
    for v in VARIANTS:
        s = summarise(v)
        if not s:
            continue
        acted = f"{s['acted']}/{s['n']} {s['acted_pct']:.0%}"
        wrote = f"{s['wrote']}/{s['n']} {s['wrote_pct']:.0%}"
        ttfa = f"{s['ttfa_med']:.1f}" if s["ttfa_med"] else "-"
        print(f"{s['variant']:<12} {s['n']:>4} {s['cases']:>6} {s['errors']:>4} "
              f"{acted:>12} {wrote:>12} {ttfa:>7} {s['reason_med']:>8}")
    print("\n⚠ ONE TURN, not a whole task. A win here is a necessary, not sufficient, condition "
          "for moving the run-level test-author row.")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    h = sub.add_parser("harvest"); h.add_argument("--limit", type=int, default=1500)
    r = sub.add_parser("run")
    r.add_argument("--variant", default="baseline")
    r.add_argument("--kind", default="test-author")
    r.add_argument("--n", type=int, default=3)
    r.add_argument("--limit", type=int, default=10)
    r.add_argument("--workers", type=int, default=1)
    r.add_argument("--timeout", type=int, default=300)
    r.add_argument("--hard", action="store_true")
    t = sub.add_parser("triage")
    t.add_argument("--n", type=int, default=5)
    t.add_argument("--limit", type=int, default=20)
    t.add_argument("--workers", type=int, default=1)
    t.add_argument("--kind", default="test-author")
    sub.add_parser("report")
    a = ap.parse_args()
    if a.cmd == "harvest":
        return harvest(a.limit)
    if a.cmd == "run":
        return run(a.variant, a.kind, a.n, a.limit, a.workers, a.timeout, a.hard)
    if a.cmd == "triage":
        return run("baseline", a.kind, a.n, a.limit, a.workers, 300, False)
    if a.cmd == "report":
        return report()
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
