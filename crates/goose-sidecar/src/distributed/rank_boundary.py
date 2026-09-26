# goose distributed tensor rank: where a chat request's stable prefix ends, so the prompt cache can
# keep it (Q-142). Pure stdlib; concatenated after rank_state.py, before rank_tool_stream.py and
# rank_wrapper.py, which installs it.
#
# goose ends every agent request with a turn-context block (current time, working directory,
# ledger) appended to the newest tool results or user message (agents/moim.rs, Q-94), and on the
# NEXT request that block is gone from that message and a new one rides the newest. The 27B is a
# hybrid: its linear-attention layers keep an ArraysCache that cannot be trimmed, so mlx_lm 0.31.3's
# LRU prompt cache reuses an entry only when the new prompt holds its WHOLE key. Upstream stores
# one entry per request at the end of generation (prompt + answer) — its key holds the block the
# next request dropped — plus, on a request that ends on a user message, the system prompt. E2E #3c
# (3.0.51, 20:51–20:56): three agent calls of 58,379 / 58,774 / 59,600 tokens each read exactly
# 31,385 from cache (the system prompt) and re-read ~27k.
#
# The single engine (Rapid-MLX lz.6+) and the fork's pipeline serve both take goose's
# `rapid_mlx_transient_tail`: the client names the exact text its last user or tool message ends on
# that the next request drops, and the engine snapshots its cache at the token the prompt stops
# agreeing with the same conversation minus that text plus a probe answer
# (`BatchedEngine._compute_prefix_boundary`). This is that rule, reused as is, for mlx_lm: the
# boundary becomes a segment end, and mlx_lm's own segment snapshots (server.py `_generate`, "Save
# the caches at end of segments") store the entry the next request extends. Every rank computes it
# from the same shared request with the same tokenizer, so every rank cuts — and chunks — alike.

TRANSIENT_TAIL = "rapid_mlx_transient_tail"
# The tail may end the last TOOL message (Rapid-MLX lz.6): goose then keeps its block joined to the
# tool results instead of posting it as an empty user turn of its own (Q-94).
TRANSIENT_TAIL_ON_TOOL = "rapid_mlx_transient_tail_on_tool"
TRANSIENT_TAIL_EXTENSIONS = (TRANSIENT_TAIL, TRANSIENT_TAIL_ON_TOOL)
BOUNDARY_PROBE = "__rapid_mlx_boundary_probe__"
# The single engine's `_PREFIX_BOUNDARY_REPLAY_TOKENS`, kept so all three ways snapshot at the same
# token of the same prompt.
BOUNDARY_REPLAY_TOKENS = 8


class TailIgnored(ValueError):
    """The request names a transient tail this rank cannot place; it is served, not snapshotted."""


def stable_messages(messages, tail):
    """`messages` with `tail` removed from the end of the last user or tool message (a copy of
    that message; the others are shared). A user message that held nothing but the tail goes
    whole, as the single engine drops it."""
    index = next(
        (i for i in range(len(messages) - 1, -1, -1) if messages[i].get("role") in ("user", "tool")),
        None,
    )
    if index is None:
        raise TailIgnored("the request has no user or tool message to end on it")
    content = messages[index].get("content")
    if not isinstance(content, str) or not content.endswith(tail):
        raise TailIgnored(
            f"it is not the exact end of message #{index} ({messages[index].get('role')}, "
            f"{len(content) if isinstance(content, str) else type(content).__name__} chars)"
        )
    stable = content[: len(content) - len(tail)]
    if stable == "" and messages[index].get("role") == "user":
        return list(messages[:index])
    return [*messages[:index], {**messages[index], "content": stable}]


def common_prefix(a, b):
    n = 0
    for x, y in zip(a, b):
        if x != y:
            break
        n += 1
    return n


def stable_boundary(prompt, future):
    """The token the stable prefix ends at: where `prompt` (the request as sent) stops agreeing
    with `future` (its stable messages plus a probe answer, rendered without a generation
    prompt), less the single engine's replay margin. 0 = nothing worth keeping."""
    return max(0, common_prefix(prompt, future) - BOUNDARY_REPLAY_TOKENS)


def cut_at_boundary(segments, segment_types, boundary):
    """mlx_lm's segments with a segment ending at `boundary` (typed "user", as mlx_lm types a
    conversation prefix), and everything after it one segment of the last one's type: past the
    boundary the prompt holds the text the client declared volatile, so no snapshot there can
    ever be reused. Unchanged when the boundary is not strictly inside the prompt."""
    total = sum(len(s) for s in segments)
    if not 0 < boundary < total:
        return segments, segment_types
    prompt = [token for segment in segments for token in segment]
    ends, types, end = [], [], 0
    for segment, kind in zip(segments, segment_types):
        end += len(segment)
        if end > boundary:
            break
        ends.append(end)
        types.append(kind)
    if not ends or ends[-1] != boundary:
        ends.append(boundary)
        types.append("user")
    ends.append(total)
    types.append(segment_types[-1])
    return [prompt[a:b] for a, b in zip([0, *ends[:-1]], ends)], types
