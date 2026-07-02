"""OperatorBrain — the EXPLORATORY-mode brain with NO API key: Claude Code (the operator driving
this session) IS the brain, for free. Instead of calling an SDK, `complete()` writes the request to
a handshake file under runs/operator/ and blocks until the operator writes the answer back — exactly
like the existing ASK gate. Select with SWARMGYM_PROVIDER=operator.

Handshake protocol (one dir, two files per call):
  runs/operator/req-<n>.json   {id, role, system, user, max_tokens, ts}   <- harness writes, operator reads
  runs/operator/resp-<n>.txt   <the operator's raw completion>            <- operator writes, harness reads
A PENDING.md index lists open requests so the operator (Claude Code) sees what to answer next.
"""

from __future__ import annotations

import json
import time
from pathlib import Path
from typing import Optional

from .base import Brain

_HANDSHAKE_DIR = Path("runs/operator")
_POLL_SECS = 3.0
_DEFAULT_TIMEOUT_SECS = 1800  # 30 min: the operator loop answers within a wake cycle; then fail loud


class OperatorTimeout(RuntimeError):
    pass


class OperatorBrain(Brain):
    def __init__(self, role: str = "operator", handshake_dir: Optional[Path] = None,
                 timeout_secs: int = _DEFAULT_TIMEOUT_SECS):
        super().__init__(f"operator:{role}")
        self.role = role
        self.dir = Path(handshake_dir) if handshake_dir else _HANDSHAKE_DIR
        self.timeout_secs = timeout_secs
        self._n = 0

    def _next_id(self) -> int:
        self.dir.mkdir(parents=True, exist_ok=True)
        existing = [int(p.stem.split("-")[1]) for p in self.dir.glob("req-*.json")
                    if p.stem.split("-")[1].isdigit()]
        self._n = (max(existing) + 1) if existing else 1
        return self._n

    def _write_pending(self, n: int, system: str, user: str) -> None:
        idx = self.dir / "PENDING.md"
        line = (f"- [ ] req-{n} ({self.role}) — answer by writing runs/operator/resp-{n}.txt\n"
                f"      ask: {user.strip()[:200].replace(chr(10), ' ')}\n")
        with idx.open("a") as f:
            f.write(line)

    def complete(self, system: str, user: str, max_tokens: int = 4096) -> str:
        n = self._next_id()
        req = self.dir / f"req-{n}.json"
        resp = self.dir / f"resp-{n}.txt"
        req.write_text(json.dumps(
            {"id": n, "role": self.role, "system": system, "user": user,
             "max_tokens": max_tokens, "ts": time.time()}, indent=2))
        self._write_pending(n, system, user)
        deadline = time.time() + self.timeout_secs
        while time.time() < deadline:
            if resp.exists():
                text = resp.read_text()
                if text.strip():
                    return text
            time.sleep(_POLL_SECS)
        raise OperatorTimeout(
            f"operator did not answer req-{n} within {self.timeout_secs}s "
            f"(write {resp} to unblock). This is EXPLORATORY mode — the operator (Claude Code) is the brain.")
