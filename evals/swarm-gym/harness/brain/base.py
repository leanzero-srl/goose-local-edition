"""Brain transport: a model-agnostic LLM call + robust JSON extraction. Role prompts live in the
generator / judge / tweaker modules, which call `json_call`."""

from __future__ import annotations

import json
import re
from typing import Any, Dict

_FENCED = re.compile(r"```(?:json)?\s*(\{.*?\})\s*```", re.DOTALL)
_BARE = re.compile(r"\{.*\}", re.DOTALL)


def extract_json(text: str) -> Dict[str, Any]:
    """Pull the first JSON object out of model output (fenced block preferred, else first {...})."""
    m = _FENCED.search(text)
    candidate = m.group(1) if m else None
    if candidate is None:
        m = _BARE.search(text)
        candidate = m.group(0) if m else None
    if candidate is None:
        raise ValueError(f"no JSON object in model output: {text[:300]!r}")
    return json.loads(candidate)


class Brain:
    def __init__(self, name: str):
        self.name = name

    def complete(self, system: str, user: str, max_tokens: int = 4096) -> str:
        raise NotImplementedError

    def json_call(self, system: str, user: str, max_tokens: int = 4096) -> Dict[str, Any]:
        raw = self.complete(
            system + "\n\nRespond with exactly ONE JSON object and no prose.", user, max_tokens
        )
        return extract_json(raw)
