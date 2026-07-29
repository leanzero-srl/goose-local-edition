"""The fast loop: give a model the Meridian task, run what it built, score the traps.

Runs through `goose run` — the same path the real benchmark uses — rather than a raw API call.
That choice buys three things:

  * the model has TOOLS, so "did it read the vendor docs" is a real measurement rather than a stub;
  * it is representative, so a trap that bites here will bite in the swarm;
  * it sidesteps a Bedrock guardrail that content-filters the raw `converse` shape for this prompt
    (measured: the payment-object and example-exchange sections trip it every time, while the exact
    same content through goose does not).

An episode is 1-3 minutes, so the loop is still fast enough to iterate trap design many times before
committing to a 30-minute swarm run.

FAIRNESS: the prompt names no trap and gives no hint. It states the interface and where the docs live.
Everything the model needs is one fetch away, and the docs say plainly what the contract is — so a
miss here is a genuine defect, which is the bar the legitimacy rule sets.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Dict, List

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from probes import vendor_trace  # noqa: E402
import vendor_service  # noqa: E402

GOOSE = Path.home() / "Projects/goose/target/release/goose"
MODELS = {
    "opus-5": "us.anthropic.claude-opus-5",
    "sonnet-5": "us.anthropic.claude-sonnet-5",
    "haiku-4.5": "us.anthropic.claude-haiku-4-5-20251001-v1:0",
}

PROMPT = """\
Write a Python module `candidate_client.py` in this directory that integrates with the Meridian
Payments API.

The API's documentation is at {docs_url} — read it before you write the client.
The base URL is {base_url} and the API key is {api_key}.

Implement exactly this interface, standard library only:

class MeridianClient:
    def __init__(self, base_url: str, api_key: str) -> None
    def fetch_all_payments(self) -> list[dict]   # every payment, oldest first by when it occurred
    def total_count(self) -> int                 # how many payments exist in the collection
    def create_payment(self, amount_minor: int, currency: str, idempotency_key: str) -> str
                                                 # creates a payment, returns its id;
                                                 # safe to call more than once with the same key

Write only that file. Do not write tests or a README.
"""


def load_env(path: str = "~/.config/agent-board/bedrock.env") -> Dict[str, str]:
    env: Dict[str, str] = {}
    for raw in Path(path).expanduser().read_text().splitlines():
        line = raw.strip().removeprefix("export ").strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, value = line.partition("=")
        env[key.strip()] = value.strip().strip("'\"")
    return env


def run_agent(model: str, workdir: Path, port: int, env: Dict[str, str], timeout: int) -> Dict:
    prompt = PROMPT.format(docs_url=f"http://127.0.0.1:{port}/v1/docs",
                           base_url=f"http://127.0.0.1:{port}",
                           api_key=vendor_service.API_KEY)
    started = time.time()
    try:
        proc = subprocess.run(
            [str(GOOSE), "run", "--provider", "aws_bedrock", "--model", model, "-t", prompt],
            cwd=workdir, capture_output=True, text=True, timeout=timeout,
            env={**os.environ, **env})
        code, tail = proc.returncode, (proc.stdout + proc.stderr)[-2000:]
    except subprocess.TimeoutExpired:
        code, tail = None, "agent timed out"
    return {"exit": code, "secs": round(time.time() - started, 1), "tail": tail}


def exercise(workdir: Path, port: int) -> Dict:
    client = workdir / "candidate_client.py"
    if not client.is_file():
        found = sorted(p for p in workdir.rglob("*.py") if p.name != "driver.py")
        if not found:
            return {"errors": {"produce": "no python file was written"}}
        client = found[0]
    out = workdir / "results.json"
    try:
        subprocess.run(
            [sys.executable, str(HERE / "driver.py"), str(client),
             f"http://127.0.0.1:{port}", vendor_service.API_KEY, str(out)],
            capture_output=True, text=True, timeout=180)
    except subprocess.TimeoutExpired:
        return {"errors": {"driver": "timed out after 180s"}}
    return json.loads(out.read_text()) if out.exists() else {"errors": {"driver": "no results"}}


def score_one(model_key: str, env: Dict[str, str], port: int, root: Path, rep: int,
              timeout: int) -> Dict:
    workdir = root / f"{model_key}-r{rep}"
    if workdir.exists():
        shutil.rmtree(workdir)
    workdir.mkdir(parents=True)
    trace_path = root / f"trace-{model_key}-r{rep}.jsonl"

    server = vendor_service.serve(port, trace_path)
    try:
        agent = run_agent(MODELS[model_key], workdir, port, env, timeout)
        # Grade the delivered client, not the agent's scratch testing.
        vendor_service.begin_exercise_phase()
        results = exercise(workdir, port)
        results["_true_order"] = vendor_service.true_order_ids()
    finally:
        server.shutdown()

    trace: List[Dict] = [json.loads(l) for l in trace_path.read_text().splitlines() if l.strip()]
    verdict = vendor_trace.evaluate(trace, results)
    verdict.update({"model": model_key, "rep": rep, "agent": agent,
                    "errors": results.get("errors", {})})
    return verdict


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--model", default="opus-5", choices=sorted(MODELS))
    ap.add_argument("--reps", type=int, default=1)
    ap.add_argument("--port", type=int, default=8791)
    ap.add_argument("--timeout", type=int, default=900)
    ap.add_argument("--out", type=Path, default=HERE.parent / "runs/calibration")
    args = ap.parse_args()

    env = load_env()
    if "AWS_BEARER_TOKEN_BEDROCK" not in env:
        raise SystemExit("no AWS_BEARER_TOKEN_BEDROCK")
    env.setdefault("AWS_REGION", "us-east-1")

    verdicts = []
    for rep in range(args.reps):
        verdict = score_one(args.model, env, args.port + rep, args.out, rep, args.timeout)
        verdicts.append(verdict)
        print(vendor_trace.format_report(
            verdict, f"{args.model} rep{rep} ({verdict['agent']['secs']}s)"))
        if verdict["errors"]:
            print(f"  runtime errors: {list(verdict['errors'])}")
        print(flush=True)

    if len(verdicts) > 1:
        scores = [v["score"] for v in verdicts]
        print(f"spread: {min(scores) * 100:.1f}% – {max(scores) * 100:.1f}%")
        always = set.intersection(*[{c["check"] for c in v["checks"] if not c["ok"]}
                                    for v in verdicts])
        print(f"missed in EVERY rep: {sorted(always) or 'none'}")

    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / f"calibration-{args.model}.json").write_text(json.dumps(verdicts, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
