"""The harness brain: the AI that generates tasks, vibes follow-ups, judges, and tweaks."""

from __future__ import annotations

import os
from typing import Any, Dict

from .base import Brain


def make_brain(cfg: Dict[str, Any], role: str = "generate") -> Brain:
    """Build the brain for a role. role ∈ {generate, judge, tweak}; judge uses the strongest model.
    SWARMGYM_PROVIDER overrides the configured provider (claude|local) without editing config.yaml."""
    b = cfg["brain"]
    provider = os.environ.get("SWARMGYM_PROVIDER", b.get("provider", "claude"))
    if provider == "local":
        from .local_brain import LocalBrain

        return LocalBrain(b["local_base_url"], b["local_model"])
    from .claude_brain import ClaudeBrain

    model = b["judge_model"] if role == "judge" else b["claude_model"]
    return ClaudeBrain(model)
