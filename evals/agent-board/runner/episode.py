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
import signal
import subprocess
import sys
import time
from pathlib import Path
from typing import Dict, List, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "probes"))
import build  # noqa: E402
import common  # noqa: E402
import fleet  # noqa: E402
import repair  # noqa: E402
import testwrite  # noqa: E402

BOARD = Path(__file__).resolve().parents[1]
CAPS = {"swarm": 3600, "single": 1200}

# The grader is chosen by the fixture's own `vertical`, never by which module happened to be
# imported first. Grading a test-writing episode with the repair probe would look for a target_test
# that does not exist and score a working suite zero.
PROBES = {"repair": repair, "testwrite": testwrite, "build": build}


def probe_for(meta: Dict) -> object:
    vertical = meta.get("vertical")
    if vertical not in PROBES:
        raise SystemExit(f"no probe for vertical {vertical!r}; known: {sorted(PROBES)}")
    return PROBES[vertical]


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


CLOUD_PROVIDER_MARKERS = ("aws_bedrock", "anthropic", "openai", "databricks", "gcpvertexai")


def running_engines(exclude_pid: Optional[int] = None) -> List[int]:
    """PIDs of goose engines currently holding the LOCAL fleet.

    A cloud episode runs `goose run` too, so matching on the binary alone would make every local
    tick queue behind Bedrock traffic it does not share a single GPU with. The provider flag in the
    child's own argv is what distinguishes them.
    """
    try:
        out = subprocess.run(["pgrep", "-f", r"goose (swarm run|run) "],
                             capture_output=True, text=True, timeout=15).stdout
    except (subprocess.SubprocessError, OSError):
        return []
    holding = []
    for token in out.split():
        if not token.isdigit():
            continue
        pid = int(token)
        if pid in (exclude_pid, os.getpid()):
            continue
        # argv is read PER PID. A prompt containing newlines splits one process across several
        # lines of `pgrep -fl` output, which hid the --provider flag on a line the parser skipped
        # and made every cloud episode look like it was holding the fleet.
        try:
            argv = subprocess.run(["ps", "-o", "command=", "-p", token],
                                  capture_output=True, text=True, timeout=10).stdout
        except (subprocess.SubprocessError, OSError):
            argv = ""
        if any(marker in argv for marker in CLOUD_PROVIDER_MARKERS):
            continue
        holding.append(pid)
    return holding


LOCAL_PROVIDERS = ("lmstudio", "ollama", "localai", "llama")


def uses_local_fleet(target: str, provider: Optional[str]) -> bool:
    """Only entrants served by the local fleet can contend for it.

    A Bedrock episode shares nothing with a running swarm, so refusing it would serialise the whole
    board for no measurement benefit — the guard has to know WHICH resource is scarce.
    """
    return target == "swarm" or (provider or "").lower().startswith(LOCAL_PROVIDERS)


def assert_fleet_idle(allow_busy: bool, target: str, provider: Optional[str],
                      wait_secs: int = 0) -> None:
    """A run against a busy fleet measures CONTENTION, not capability.

    This exists because a killed supervisor left an orphaned `goose swarm` engine running for 33
    minutes, parented to launchd, quietly competing for the same three nodes as the next episode.
    Nothing would have flagged it — the numbers would just have been worse, and wrongly attributed
    to the model.

    For an unattended board, WAITING beats refusing: an overnight run that aborts because something
    else briefly held the fleet has thrown away the night. Refusing is the interactive default.
    """
    if not uses_local_fleet(target, provider):
        return
    deadline = time.monotonic() + wait_secs
    while running_engines() and time.monotonic() < deadline:
        time.sleep(15)
    busy = running_engines()
    if not busy:
        return
    message = (f"another goose engine is already running (pid {', '.join(map(str, busy))}). "
               f"An episode started now measures contention, not capability.")
    if allow_busy:
        print(f"[warn] {message} Continuing because --allow-busy was passed.")
        return
    raise SystemExit(f"[refused] {message}\n"
                     f"          Kill it, or pass --allow-busy to measure anyway.")


def terminate_group(proc: subprocess.Popen) -> None:
    """Kill the engine AND its children. subprocess's own timeout kills only the direct child, so a
    swarm's workers would survive as orphans and poison the next episode."""
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
    except (ProcessLookupError, PermissionError, OSError):
        proc.kill()
    try:
        proc.wait(timeout=20)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
        except (ProcessLookupError, PermissionError, OSError):
            proc.kill()


def extract_claim(episode_dir: Path) -> Dict:
    """The CLAIM side of the honesty card — what the run said about itself.

    Only the swarm emits a STRUCTURED claim (`complete_result{passed}`). A single-agent run exits 0
    whether or not it worked, and reading success out of the model's closing prose would be exactly
    the self-report this benchmark refuses to treat as evidence. So honesty is marked NOT COMPUTABLE
    for those entrants rather than inferred, and the card reports the coverage instead of hiding it.
    """
    log = episode_dir / "run.jsonl"
    if not log.is_file():
        return {"available": False,
                "reason": "single-agent goose emits no structured completion claim"}
    claimed, finished = None, False
    for raw in log.read_text(errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            event = json.loads(raw)
        except json.JSONDecodeError:
            continue
        if event.get("event") == "complete_result":
            claimed = event.get("passed")
        elif event.get("event") == "run_finished":
            finished = True
    if claimed is None:
        return {"available": False, "run_finished": finished,
                "reason": "no complete_result in the stream (gate off, or the run never got there)"}
    return {"available": True, "claimed_pass": bool(claimed), "run_finished": finished}


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
                env_file: Optional[str] = None, allow_busy: bool = False,
                wait_for_fleet: int = 0, nodes: Optional[int] = None) -> Dict:
    meta = common.load_meta(fixture)
    probe_mod = probe_for(meta)
    tag = label or model or ("local-swarm" if target == "swarm" else "local-single")
    episode_id = f"{fixture.name}__{target}__{tag}__r{rep}".replace("/", "-")
    episode_dir = out_root / episode_id
    done = episode_dir / "episode.json"
    if done.is_file():
        record = json.loads(done.read_text())
        if record.get("complete"):
            print(f"[skip] {episode_id} already complete (score {record['score']})")
            return record

    assert_fleet_idle(allow_busy, target, provider, wait_for_fleet)

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
    # Fleet scaling: an entrant declaring `nodes` runs against exactly that many devices, and the
    # user's pool is restored afterwards whatever happens.
    pool_ctx = fleet.sized(binary, nodes) if (nodes and target == "swarm") else None
    active_nodes = pool_ctx.__enter__() if pool_ctx else None
    # start_new_session gives the engine its own process group, so a timeout can take its workers
    # down with it instead of leaving them orphaned against the fleet.
    proc = subprocess.Popen(cmd, cwd=workspace, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, env=child_env, start_new_session=True)
    try:
        stdout, stderr = proc.communicate(timeout=cap)
        exit_code = proc.returncode
    except subprocess.TimeoutExpired:
        timed_out, exit_code = True, None
        terminate_group(proc)
        stdout, stderr = proc.communicate()
    finally_err = None
    if pool_ctx:
        try:
            pool_ctx.__exit__(None, None, None)
        except Exception as exc:  # restoring the pool must never mask the episode result
            finally_err = str(exc)
    wall = round(time.monotonic() - started, 1)

    (episode_dir / "stdout.txt").write_text(stdout or "")
    (episode_dir / "stderr.txt").write_text(stderr or "")

    crashed = exit_code not in (0, None) and not timed_out
    graded_root = effective_workspace(workspace)
    if graded_root != workspace:
        (episode_dir / "relocated.txt").write_text(str(graded_root))
    probe = probe_mod.grade(fixture, graded_root)
    claim = extract_claim(episode_dir)

    record = {
        "episode_id": episode_id,
        "complete": True,
        "fixture": fixture.name,
        "vertical": meta["vertical"],
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
        "crashed": crashed,
        "graded_root": str(graded_root),
        "nodes": nodes,
        "active_nodes": active_nodes,
        "pool_restore_error": finally_err,
        # A run that never terminated did not DELIVER, however correct the tree it left behind.
        # Scoring the artifact anyway would let an agent that spins for an hour tie one that
        # finished in thirty seconds. The probe verdict is kept alongside, so "the code was right
        # but the run never stopped" stays visible instead of being silently rounded to failure.
        "score": 0.0 if (timed_out or crashed) else probe["score"],
        "artifact_score": probe["score"],
        "scored_zero_for": ("timeout" if timed_out else "crash") if (timed_out or crashed) else None,
        "probe": probe,
        "claim": claim,
        "false_green": bool(claim.get("claimed_pass")) and probe["score"] == 0.0,
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
    ap.add_argument("--nodes", type=int, help="swarm pool size for this episode (1|2|3)")
    ap.add_argument("--wait-for-fleet", type=int, default=0, metavar="SECS",
                    help="wait up to SECS for the fleet to go idle instead of refusing")
    ap.add_argument("--allow-busy", action="store_true",
                    help="measure even though another engine is running (records contention)")
    args = ap.parse_args()

    record = run_episode(args.fixture, args.target, args.rep, args.out,
                         args.provider, args.model, args.label, args.timeout,
                         args.env_file, args.allow_busy, args.wait_for_fleet, args.nodes)
    return 0 if record["complete"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
