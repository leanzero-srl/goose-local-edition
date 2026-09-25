# goose distributed tensor rank: how many tokens one request may generate. Pure stdlib; concatenated
# after rank_live.py, before rank_wrapper.py.
#
# mlx_lm.server fills an absent max_tokens with its `--max-tokens` default, 512 (server.py:1842 in
# mlx_lm 0.31.3), so every answer on the split stopped at exactly 512 tokens — a thinking model spent
# them all reasoning and goose saw "The model returned an empty response" (Q-65, 2026-09-25). goose
# sends no max_tokens on purpose (formats/openai.rs: "omitting the field lets the server use its own
# max"), so the server's own max must BE the room the launch was planned for: the context window
# preflight sized the ranks' memory for, less the prompt. That is the fork's pipeline rule too
# (pipeline_qwen4_serve.py: `budget = state.context - len(ids) - 1`, `min(requested, budget)`).


class ContextFull(ValueError):
    """The prompt leaves no room to generate inside the launch's context window."""


def generation_budget(context_window, prompt_tokens, requested):
    """The tokens a request may generate: the room left in the window after its prompt, or the
    client's own max_tokens when that is smaller. `requested` None = the client set none, so the
    request runs until the model stops or the window is full."""
    room = context_window - prompt_tokens
    if room < 1:
        raise ContextFull(
            f"This model's maximum context length is {context_window} tokens; the prompt is "
            f"{prompt_tokens} tokens, which leaves no room to generate (the split was launched for "
            f"a {context_window}-token context). Reduce the length of the messages."
        )
    return room if requested is None else min(requested, room)
