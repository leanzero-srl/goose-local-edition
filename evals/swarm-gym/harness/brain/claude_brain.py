"""Cloud brain — Anthropic Claude (reads ANTHROPIC_API_KEY). The default judge."""

from __future__ import annotations

from .base import Brain


class ClaudeBrain(Brain):
    def __init__(self, model: str):
        import anthropic

        self.client = anthropic.Anthropic()
        self.model = model
        super().__init__(f"claude:{model}")

    def complete(self, system: str, user: str, max_tokens: int = 4096) -> str:
        msg = self.client.messages.create(
            model=self.model,
            max_tokens=max_tokens,
            system=system,
            messages=[{"role": "user", "content": user}],
        )
        return "".join(b.text for b in msg.content if getattr(b, "type", None) == "text")
