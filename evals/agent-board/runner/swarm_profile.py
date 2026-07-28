"""Point the swarm engine at a different model backend, and always put the config back.

Running Claude through the engine means temporarily rewriting the user's swarm block: endpoint,
planner, and the device pool. Every change is backed up byte-for-byte and restored from that backup.

DO NOT ROUND-TRIP THIS FILE THROUGH A YAML LIBRARY. goose reads it with serde_yaml (YAML 1.2) where
`research_planning: on` is the STRING "on". pyyaml is YAML 1.1, where `on` is a BOOLEAN, so a
load/dump cycle rewrites it as `true`, deserialisation of the whole swarm block fails, and the
engine silently falls back to its baked defaults — different endpoint, different planner, different
devices, no error printed. Every cloud-swarm episode would have run on the wrong configuration and
looked entirely normal.

So this edits TEXT: it replaces three things inside the swarm block and leaves every other byte
alone. Stdlib only, no yaml dependency, nothing to round-trip.
"""

from __future__ import annotations

import argparse
import re
import shutil
from pathlib import Path
from typing import List

CONFIG = Path.home() / ".config/goose/config.yaml"
BACKUP = Path.home() / ".config/agent-board/config.yaml.before-agent-board"
GATEWAY = "http://127.0.0.1:4000"

PROFILES = {
    "cloud-haiku": "claude-haiku",
    "cloud-sonnet": "claude-sonnet",
    "cloud-opus": "claude-opus",
}


def _swarm_bounds(lines: List[str]) -> tuple[int, int]:
    start = next((i for i, line in enumerate(lines) if line.rstrip() == "swarm:"), -1)
    if start < 0:
        raise SystemExit("no `swarm:` block in the config")
    end = len(lines)
    for i in range(start + 1, len(lines)):
        if lines[i] and not lines[i][0].isspace():
            end = i
            break
    return start, end


def _replace_scalar(lines: List[str], start: int, end: int, key: str, value: str) -> None:
    pattern = re.compile(rf"^(\s\s){re.escape(key)}:\s")
    for i in range(start + 1, end):
        if pattern.match(lines[i]):
            lines[i] = f"  {key}: {value}"
            return
    lines.insert(start + 1, f"  {key}: {value}")


def _replace_devices(lines: List[str], start: int, end: int, devices: List[str]) -> None:
    head = next((i for i in range(start + 1, end) if lines[i].rstrip() == "  devices:"), -1)
    if head < 0:
        raise SystemExit("no `devices:` key inside the swarm block")
    tail = end
    for i in range(head + 1, end):
        # the next key at the swarm block's own indent ends the list
        if re.match(r"^\s\s\S", lines[i]) and not lines[i].lstrip().startswith("-"):
            tail = i
            break
    lines[head + 1:tail] = devices


def apply(profile: str, nodes: int) -> None:
    if profile not in PROFILES:
        raise SystemExit(f"unknown profile {profile!r}; known: {sorted(PROFILES)}")
    if not BACKUP.exists():
        BACKUP.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(CONFIG, BACKUP)
        print(f"backed up {CONFIG} -> {BACKUP}")

    lines = CONFIG.read_text().splitlines()
    start, end = _swarm_bounds(lines)
    suffix = PROFILES[profile]

    devices: List[str] = []
    for i in range(1, nodes + 1):
        devices += [f"  - id: n{i}-{suffix}",
                    f"    model_id: n{i}-{suffix}",
                    "    weight: 1",
                    "    enabled: true",
                    "    instances: 1",
                    "    host: gateway"]
    _replace_devices(lines, start, end, devices)

    start, end = _swarm_bounds(lines)
    _replace_scalar(lines, start, end, "endpoint", GATEWAY)
    _replace_scalar(lines, start, end, "planner_model", f"n1-{suffix}")

    CONFIG.write_text("\n".join(lines) + "\n")
    print(f"swarm -> {profile} ({nodes} workers) via {GATEWAY}")


def restore() -> None:
    if not BACKUP.exists():
        raise SystemExit(f"no backup at {BACKUP}; refusing to guess the original config")
    shutil.copy2(BACKUP, CONFIG)
    print(f"restored {CONFIG} from {BACKUP}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--apply", help=f"profile: {', '.join(sorted(PROFILES))}")
    ap.add_argument("--nodes", type=int, default=3)
    ap.add_argument("--restore", action="store_true")
    args = ap.parse_args()

    if args.restore:
        restore()
    elif args.apply:
        apply(args.apply, args.nodes)
    else:
        ap.error("pass --apply <profile> or --restore")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
