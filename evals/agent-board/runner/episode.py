"""Run ONE tick: materialise the seed, let an agent work, snapshot, grade, persist.

A tick is the atomic unit of the board — (vertical, fixture, target, rep). It is self-contained and
resumable: a finished episode writes episode.json and is never re-run, so a board interrupted at
hour nine resumes at hour nine.

Two things here are load-bearing and non-obvious:

  * The agent NEVER sees the controls or the probe. Only `seed/` is copied into the workspace.
  * A swarm run does not necessarily work where it was started — `resolve_app_root` relocates a run
    (it refuses $HOME) and records the truth in `.swarm/current-run.json`. Grading the spawn
    directory instead of the recorded one grades an empty tree and scores a working agent zero.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Dict, Optional

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "probes"))
import repair  # noqa: E402

BOARD = Path(__file__).resolve().parents[1]
CAPS = {"swarm": 3600, "single": 1200}


def goose_binary() -> Path:
    for candidate in ("target/release/goose", "target/debug/goose"):
        path = BOARD.parents[1] / candidate
        if path.is_file() and os.access(path, os.X_OK):
            return path
    found = shutil.which("goose")
    if not found:
        raise SystemExit("no goose binary found — run `just release-binary`")
    return Path(found)


def build_info(binary: Path) -> Dict[str, str]:
    try:
        out = subprocess.run([str(binary), "--version"], capture_output=True, text=True,
                             timeout=30).stdout.strip()
    except (subprocess.SubprocessError, OSError):
        out = "unknown"
    return {"binary": str(binary), "version": out}


def effective_workspace(spawn: Path) -> Path:
    """Follow the engine's own breadcrumb. Newest-file-wins is the bug this avoids."""
    marker = spawn / ".swarm/current-run.json"
    if not marker.is_file():
        return spawn
    try:
        recorded = json.loads(marker.read_text()).get("dir")
    except (json.JSONDecodeError, OSError):
        return spawn
    if not recorded:
        return spawn
    resolved = Path(os.path.expanduser(recorded))
    return resolved if resolved.is_dir() else spawn


def load_env_file(path: Optional[str]) -> Dict[str, str]:
    """Credentials come from a file OUTSIDE the repo and are never written into an episode record.

    An entrant declares `env_file`; the value is read at spawn time and passed to the child only.
    """
    if not path:
        return {}
    resolved = Path(os.path.expanduser(path))
    if not resolved.is_file():
        raise SystemExit(f"env_file not found: {resolved}")
    env: Dict[str, str] = {}
    for raw in resolved.read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        line = line.removeprefix("export ").strip()
        key, _, value = line.partition("=")
        if not key:
            continue
        value = value.strip().strip("'\"")
        # This file is READ, never sourced, so `${FOO:-bar}` arrives as literal text. Passing it
        # on produced a Bedrock "region must be a valid host label" error that read exactly like a
        # model failing the task. A credential file that needs a shell is a broken credential file.
        if "$" in value or "`" in value:
            raise SystemExit(
                f"{resolved}: {key.strip()} contains shell syntax; env files are parsed, not sourced "
                "— write a literal value")
        env[key.strip()] = value
    return env


def agent_command(target: str, prompt: str, provider: Optional[str], model: Optional[str],
                  binary: Path, episode_dir: Path) -> list[str]:
    if target == "swarm":
        cmd = [str(binary), "swarm", "run", prompt,
               "--output-format", "json",
               "--log-file", str(episode_dir / "run.jsonl")]
    else:
        cmd = [str(binary), "run", "-t", prompt]
    if provider:
        cmd += ["--provider", provider]
    if model:
        cmd += ["--model", model]
    return cmd


def run_episode(fixture: Path, target: str, rep: int, out_root: Path,
                provider: Optional[str] = None, model: Optional[str] = None,
                label: Optional[str] = None, timeout: Optional[int] = None,
                env_file: Optional[str] = None) -> Dict:
    meta = repair._load_meta(fixture)
    tag = label or model or ("local-swarm" if target == "swarm" else "local-single")
    episode_id = f"{fixture.name}__{target}__{tag}__r{rep}".replace("/", "-")
    episode_dir = out_root / episode_id
    done = episode_dir / "episode.json"
    if done.is_file():
        record = json.loads(done.read_text())
        if record.get("complete"):
            print(f"[skip] {episode_id} already complete (score {record['score']})")
            return record

    if episode_dir.exists():
        shutil.rmtree(episode_dir)
    workspace = episode_dir / "workspace"
    shutil.copytree(fixture / "seed", workspace)

    binary = goose_binary()
    prompt = (fixture / "prompt.md").read_text().strip()
    cmd = agent_command(target, prompt, provider, model, binary, episode_dir)
    cap = timeout or CAPS[target]

    child_env = dict(os.environ, **load_env_file(env_file))
    started = time.monotonic()
    timed_out = False
    try:
        proc = subprocess.run(cmd, cwd=workspace, capture_output=True, text=True, timeout=cap,
                              env=child_env)
        exit_code, stdout, stderr = proc.returncode, proc.stdout, proc.stderr
    except subprocess.TimeoutExpired as exc:
        timed_out, exit_code = True, None
        stdout = exc.stdout.decode() if isinstance(exc.stdout, bytes) else (exc.stdout or "")
        stderr = exc.stderr.decode() if isinstance(exc.stderr, bytes) else (exc.stderr or "")
    wall = round(time.monotonic() - started, 1)

    (episode_dir / "stdout.txt").write_text(stdout or "")
    (episode_dir / "stderr.txt").write_text(stderr or "")

    graded_root = effective_workspace(workspace)
    if graded_root != workspace:
        (episode_dir / "relocated.txt").write_text(str(graded_root))
    probe = repair.grade(fixture, graded_root)

    record = {
        "episode_id": episode_id,
        "complete": True,
        "fixture": fixture.name,
        "vertical": meta.get("vertical", "repair"),
        "difficulty": meta.get("difficulty"),
        "target": target,
        "provider": provider,
        "model": model,
        "label": tag,
        "rep": rep,
        "started_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "wall_secs": wall,
        "exit_code": exit_code,
        "timed_out": timed_out,
        "crashed": exit_code not in (0, None) and not timed_out,
        "graded_root": str(graded_root),
        "score": probe["score"],
        "probe": probe,
        "build": build_info(binary),
    }
    done.write_text(json.dumps(record, indent=2))
    flag = "TAMPERED " if probe["tampered"] else ""
    print(f"[tick] {episode_id}  score={probe['score']:.1f} {flag}({wall}s) — {probe['reason']}")
    return record


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fixture", required=True, type=Path)
    ap.add_argument("--target", required=True, choices=sorted(CAPS))
    ap.add_argument("--rep", type=int, default=0)
    ap.add_argument("--out", type=Path, default=BOARD / "runs")
    ap.add_argument("--provider")
    ap.add_argument("--model")
    ap.add_argument("--label")
    ap.add_argument("--timeout", type=int)
    ap.add_argument("--env-file", help="path to a credentials file OUTSIDE the repo")
    args = ap.parse_args()

    record = run_episode(args.fixture, args.target, args.rep, args.out,
                         args.provider, args.model, args.label, args.timeout,
                         args.env_file)
    return 0 if record["complete"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
