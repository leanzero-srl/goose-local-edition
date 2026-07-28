"""Prove the OpenAI-compatible gateway does not degrade Claude before any number from it is trusted.

goose is a tool-calling agent. A gateway that drops a tool definition, mangles a tool_call, or
silently rewrites arguments would cripple the cloud swarm and hand back a flattering result for the
local one — the accidental version of faking a benchmark, and the hardest kind to notice because
every number still looks plausible.

So the same request goes through BOTH paths and the answers must agree on the things that matter:
the model chose to call the tool, it chose the right one, and the arguments survived intact.

Bedrock `converse` is the ground truth here because it is what the single-agent baselines already
used; the gateway is the new, untrusted component.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Dict, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
from episode import load_env_file  # noqa: E402

PROMPT = "What is the weather in Paris? Use the get_weather tool. Do not answer from memory."
TOOL_NAME = "get_weather"
TOOL_SCHEMA = {
    "type": "object",
    "properties": {"city": {"type": "string", "description": "City name"}},
    "required": ["city"],
}


def _post(url: str, body: Dict, headers: Dict[str, str], timeout: int = 90) -> Dict:
    request = urllib.request.Request(
        url, data=json.dumps(body).encode(), method="POST",
        headers={"Content-Type": "application/json", **headers})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return json.loads(response.read())
    except urllib.error.HTTPError as exc:
        return {"__error__": f"HTTP {exc.code}", "__body__": exc.read().decode()[:400]}
    except Exception as exc:  # noqa: BLE001 - surfaced, not swallowed
        return {"__error__": str(exc)}


def native_bedrock(model: str, token: str, region: str) -> Dict:
    payload = _post(
        f"https://bedrock-runtime.{region}.amazonaws.com/model/{model}/converse",
        {
            "messages": [{"role": "user", "content": [{"text": PROMPT}]}],
            "inferenceConfig": {"maxTokens": 512},
            "toolConfig": {"tools": [{"toolSpec": {
                "name": TOOL_NAME, "description": "Look up current weather for a city",
                "inputSchema": {"json": TOOL_SCHEMA}}}]},
        },
        {"Authorization": f"Bearer {token}"})
    if "__error__" in payload:
        return {"ok": False, "detail": payload}
    blocks = payload.get("output", {}).get("message", {}).get("content", [])
    calls = [b["toolUse"] for b in blocks if "toolUse" in b]
    return {"ok": True, "called": bool(calls),
            "name": calls[0]["name"] if calls else None,
            "args": calls[0].get("input") if calls else None,
            "stop_reason": payload.get("stopReason")}


def through_gateway(base_url: str, model: str) -> Dict:
    payload = _post(
        f"{base_url.rstrip('/')}/v1/chat/completions",
        {
            "model": model,
            "messages": [{"role": "user", "content": PROMPT}],
            "max_tokens": 512,
            "tools": [{"type": "function", "function": {
                "name": TOOL_NAME, "description": "Look up current weather for a city",
                "parameters": TOOL_SCHEMA}}],
        },
        {"Authorization": "Bearer sk-noop"})
    if "__error__" in payload:
        return {"ok": False, "detail": payload}
    choice = (payload.get("choices") or [{}])[0]
    calls = (choice.get("message") or {}).get("tool_calls") or []
    args: Optional[Dict] = None
    name = None
    if calls:
        name = calls[0]["function"]["name"]
        try:
            args = json.loads(calls[0]["function"]["arguments"])
        except (json.JSONDecodeError, TypeError):
            args = {"__unparseable__": calls[0]["function"].get("arguments")}
    return {"ok": True, "called": bool(calls), "name": name, "args": args,
            "stop_reason": choice.get("finish_reason")}


def compare(native: Dict, gateway: Dict) -> Dict:
    checks = {
        "both reachable": native.get("ok") and gateway.get("ok"),
        "both chose to call a tool": native.get("called") and gateway.get("called"),
        "same tool name": native.get("name") == gateway.get("name") and native.get("name"),
        "arguments survived": (isinstance(gateway.get("args"), dict)
                               and "city" in (gateway.get("args") or {})
                               and "__unparseable__" not in (gateway.get("args") or {})),
    }
    return {"checks": checks, "passed": all(bool(v) for v in checks.values())}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--base-url", default="http://127.0.0.1:4000")
    ap.add_argument("--gateway-model", default="n1-claude-haiku")
    ap.add_argument("--bedrock-model", default="us.anthropic.claude-haiku-4-5-20251001-v1:0")
    ap.add_argument("--env-file", default="~/.config/agent-board/bedrock.env")
    args = ap.parse_args()

    env = load_env_file(args.env_file)
    token = env.get("AWS_BEARER_TOKEN_BEDROCK") or os.environ.get("AWS_BEARER_TOKEN_BEDROCK", "")
    region = env.get("AWS_REGION", "us-east-1")

    native = native_bedrock(args.bedrock_model, token, region)
    gateway = through_gateway(args.base_url, args.gateway_model)
    verdict = compare(native, gateway)

    print("PROXY FIDELITY")
    print(f"  native bedrock : {json.dumps(native)[:220]}")
    print(f"  via gateway    : {json.dumps(gateway)[:220]}")
    for name, ok in verdict["checks"].items():
        print(f"    {'OK  ' if ok else 'FAIL'} {name}")
    print("GATEWAY TRUSTED" if verdict["passed"] else
          "GATEWAY NOT TRUSTED — a cloud swarm run through it would understate Claude")
    return 0 if verdict["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
