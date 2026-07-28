"""Size the swarm pool for an episode, and always put it back.

The fleet-scaling card asks the one question a local-agent product actually has to answer: what does
a second and third machine buy you? Answering it means running the SAME prompts and the SAME probes
at pool sizes 1, 2 and 3.

The pool lives in the user's own config, so every change here is temporary and restored in a finally
block. Leaving someone's fleet half-disabled because a benchmark exited badly is not acceptable, and
a run that silently inherits the WRONG pool size measures nothing at all.
"""

from __future__ import annotations

import re
import subprocess
from contextlib import contextmanager
from pathlib import Path
from typing import Dict, List

STATE_LINE = re.compile(r"^\s+(enabled|disabled)\s+(\S+)\s")


def read_pool(binary: Path) -> Dict[str, bool]:
    """{device_id: enabled}. Order is the engine's own, so a given N always picks the same nodes."""
    out = subprocess.run([str(binary), "swarm", "pool", "show"],
                         capture_output=True, text=True, timeout=60).stdout
    pool: Dict[str, bool] = {}
    for line in out.splitlines():
        match = STATE_LINE.match(line)
        if match:
            pool[match.group(2)] = match.group(1) == "enabled"
    return pool


def _set(binary: Path, device: str, enabled: bool) -> None:
    subprocess.run([str(binary), "swarm", "pool", "enable" if enabled else "disable", device],
                   capture_output=True, text=True, timeout=60)


def apply_size(binary: Path, nodes: int) -> List[str]:
    """Enable exactly the first `nodes` devices, disable the rest. Returns the active ids."""
    pool = read_pool(binary)
    ids = list(pool)
    if nodes > len(ids):
        raise SystemExit(f"asked for {nodes} nodes but the pool has {len(ids)}")
    active = ids[:nodes]
    for device in ids:
        _set(binary, device, device in active)
    return active


@contextmanager
def sized(binary: Path, nodes: int):
    """Run with exactly `nodes` devices enabled, then restore whatever was there before."""
    original = read_pool(binary)
    try:
        yield apply_size(binary, nodes)
    finally:
        for device, was_enabled in original.items():
            _set(binary, device, was_enabled)
