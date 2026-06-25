"""Invoke `goose swarm run --output-format json` in an isolated, kept workspace."""

from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path
from typing import Dict, List, Optional

from .contracts import RunResult, TaskSpec


def run_swarm(
    binary: Path,
    task: TaskSpec,
    endpoint: str,
    mcp: List[str],
    timeout: int,
    env_extra: Optional[Dict[str, str]] = None,
) -> RunResult:
    workspace = Path(task.workspace)
    workspace.mkdir(parents=True, exist_ok=True)

    env = dict(os.environ)
    env["LMSTUDIO_HOST"] = endpoint
    env.setdefault("LMSTUDIO_API_KEY", "lm-studio")
    if env_extra:
        env.update(env_extra)

    cmd = [str(binary), "swarm", "run", task.prompt, "--output-format", "json"]
    for m in mcp:
        cmd += ["--mcp", m]

    try:
        proc = subprocess.run(
            cmd,
            cwd=str(workspace),
            env=env,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        stdout, stderr, code = proc.stdout, proc.stderr, proc.returncode
    except subprocess.TimeoutExpired as e:
        out, err = e.stdout or "", e.stderr or ""
        if isinstance(out, bytes):
            out = out.decode(errors="replace")
        if isinstance(err, bytes):
            err = err.decode(errors="replace")
        stdout, stderr, code = out, err + "\n[TIMEOUT]", 124

    stderr_tail = "\n".join(stderr.splitlines()[-20:])
    try:
        rr = RunResult.model_validate(json.loads(stdout))
    except Exception as e:
        rr = RunResult(stderr_tail=f"{stderr_tail}\n[json parse failed: {e}] stdout head: {stdout[:300]}")
    rr.exit_code = code
    if not rr.stderr_tail:
        rr.stderr_tail = stderr_tail

    swarmdir = workspace / ".swarm"
    if swarmdir.exists():
        logs = sorted(swarmdir.glob("run-*.jsonl"), key=lambda p: p.stat().st_mtime)
        if logs:
            rr.jsonl_path = str(logs[-1])
    return rr
