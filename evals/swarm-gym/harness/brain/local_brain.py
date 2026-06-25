"""Local brain — Qwen via the LM Studio OpenAI-compatible endpoint. Self-contained, no cloud."""

from __future__ import annotations

from .base import Brain


class LocalBrain(Brain):
    def __init__(self, base_url: str, model: str):
        import openai

        self.client = openai.OpenAI(base_url=base_url, api_key="lm-studio")
        self.model = model
        super().__init__(f"local:{model}")

    def complete(self, system: str, user: str, max_tokens: int = 4096) -> str:
        # Qwen3.6 is a reasoning model: thinking goes to reasoning_content and can consume the whole
        # budget, leaving content empty. Give generous headroom, and fall back to reasoning_content.
        r = self.client.chat.completions.create(
            model=self.model,
            max_tokens=max(max_tokens, 16000),
            messages=[
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        )
        m = r.choices[0].message
        content = (m.content or "").strip()
        if not content:
            content = getattr(m, "reasoning_content", None) or ""
        return content
