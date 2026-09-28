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
#
# Keeping the entry is half of it; the cache must also not evict it (Q-182, `pop_keeping_newest_
# prefix` below, installed by the wrapper on a spec that asks for `keep_newest_prefix`; Q-294's
# `KeptEntry` since `keep_conversation_prefix`). The chat's stable head — its system prompt and
# tools — is cut and kept the same way (Q-347, `cut_at_head`, on a spec that asks for
# `keep_stable_head`), so a request whose messages changed (a compaction) still reads it.

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


# Q-182 (E2E #3h, 3.0.59): every full miss (cache 0 of 71–96k tokens, 4–7 min of prefill each)
# followed the prompt cache DROPPING the boundary entry the next agent request extends — goose's two
# consecutive requests were byte-identical up to the tail (15:52:12 → 15:59:24: 81 tools, messages
# 0..130 equal, message 131 differing only by its `<turn-context>` block). A tool-label helper
# joined the agent's batch; mlx_lm pads every row to the longest, so two rows at 95,514 tokens
# charge 14.1 of the 17.3 GB KV plan and the live bound trimmed the cache to ~3.2 GB. mlx_lm
# 0.31.3's eviction (`CacheOrder.pop`) picks a TYPE by entry counts — assistant while there are at
# least as many assistant entries as user ones, else user while there are at least as many user as
# system — and the oldest of it; at 15:52:11 it had kept 3.29 GB of assistant entries (the end of
# the previous agent call — prompt + answer, whose key holds the tail the next request drops, so a
# hybrid cache can never extend it) and 0 user entries. Every miss of the run shows the same shape
# (15:02, 15:12, 15:18, 15:25, 15:35: user 0 or one 0.08 GB helper segment, assistant 2.7–5.5 GB).


def pop_keeping_newest_prefix(order, upstream_pop):
    """mlx_lm's eviction (`upstream_pop(order)`, order = LRUPromptCache.CacheOrder) with the newest
    "user" entry — the stable prefix the latest conversation request left (`cut_at_boundary` types
    it "user"), the one its next request extends — held back while any other entry remains. It is
    still evicted when it is the last entry and the bound still needs room."""
    prefixes = order._lrus["user"]
    if not prefixes or len(order) == 1:
        return upstream_pop(order)
    newest = prefixes.pop()
    try:
        return upstream_pop(order)
    finally:
        prefixes.append(newest)


def keep_newest_prefix(cache_order):
    """Install `pop_keeping_newest_prefix` on mlx_lm's `LRUPromptCache.CacheOrder`."""
    upstream_pop = cache_order.pop

    def pop(self):
        return pop_keeping_newest_prefix(self, upstream_pop)

    cache_order.pop = pop


# Q-294 (E2E #3o, 3.0.66): "the newest user entry" is not always the conversation's. mlx_lm types
# the context segment of EVERY request that ends on a user message "user" — goose's end-of-turn
# helpers (fact checks, labels) included — so a helper whose prompt read past its own context
# segment became the newest "user" entry and took the prefix's protection. Turn 7 (06:08:30Z):
# eight helper rows (1,248–23,686 tokens) shared a batch; at 06:08:32 the cache still held the
# agent's stable prefix (user 2 sequences, 3.96 GB); at the agent's next fetch (06:09:24) it held
# four helper segments (user 4 sequences, 0.47 GB) and no prefix — 108,801 tokens read cold. The
# conversation prefix is now named by what cut it: the stable boundary of a request that names its
# transient tail (only goose's agent requests do), recorded when `_tokenize` cuts it and matched
# when the cache inserts that key.


def prefix_key(tokens):
    return len(tokens), hash(tuple(tokens))


class KeptEntry:
    """A cache entry the rank keeps while anything else is left to evict: the key list the cache
    holds (compared by identity) and its bytes, named by the key a cut recorded (`cut`) and
    matched when the cache inserts that key (`inserted`). Every rank tracks it over the same
    requests and inserts, so every rank keeps the same entry. Two are kept: the conversation's
    stable prefix (Q-294) and the chat's stable head (Q-347, below)."""

    def __init__(self):
        self.cut_keys = {}
        self.tokens = None
        self.nbytes = 0

    def cut(self, prefix):
        """`_tokenize` cut a segment at the end of `prefix` (its tokens) for this entry."""
        length, digest = prefix_key(prefix)
        self.cut_keys[length] = digest

    def inserted(self, tokens, nbytes):
        """The cache is inserting `tokens`: a key this entry's cut named becomes the entry."""
        if len(tokens) not in self.cut_keys:
            return
        length, digest = prefix_key(tokens)
        if self.cut_keys[length] == digest:
            del self.cut_keys[length]
            self.tokens = tokens
            self.nbytes = nbytes

    def forget(self):
        self.tokens = None
        self.nbytes = 0


def located(order, tokens):
    """The (lru, item) of mlx_lm's `CacheOrder` whose key IS `tokens`, or None."""
    for lru in order._lrus.values():
        for item in lru:
            if item[1] is tokens:
                return lru, item
    return None


def forget_dropped(order, kept):
    """Forget each kept entry whose key the cache no longer holds (replaced by an equal key, or
    dropped as the prefix of a longer trimmable entry — mlx_lm's `insert_cache` pops those
    without going through the eviction order), so no admission reserves room for it."""
    for entry in kept:
        if entry.tokens is not None and located(order, entry.tokens) is None:
            entry.forget()


def without(lru, item):
    kept = [other for other in lru if other is not item]
    lru.clear()
    lru.extend(kept)


def pop_keeping(order, upstream_pop, kept):
    """mlx_lm's eviction (`upstream_pop(order)`, order = LRUPromptCache.CacheOrder) with the
    `kept` entries — most precious first — held back while any other entry remains, in place, so
    the LRU order stands. When only kept entries are left and the bound still needs room, the
    least precious goes first."""
    forget_dropped(order, kept)
    held = []
    for entry in kept:
        if entry.tokens is None:
            continue
        lru, item = located(order, entry.tokens)
        if all(item is not other for _, _, other in held):
            held.append((entry, lru, item))
    if not held:
        return upstream_pop(order)
    if len(order) == len(held):
        _, lru, item = held[-1]
        without(lru, item)
        for entry in kept:
            if entry.tokens is item[1]:
                entry.forget()
        return item
    saved = {kind: list(lru) for kind, lru in order._lrus.items()}
    for _, lru, item in held:
        without(lru, item)
    popped = None
    try:
        popped = upstream_pop(order)
        return popped
    finally:
        for kind, items in saved.items():
            lru = order._lrus[kind]
            lru.clear()
            lru.extend(item for item in items if item is not popped)


def keep_entries(cache_order, *kept):
    """Install `pop_keeping` of `kept` on mlx_lm's `LRUPromptCache.CacheOrder`."""
    upstream_pop = cache_order.pop

    def pop(self):
        return pop_keeping(self, upstream_pop, kept)

    cache_order.pop = pop


# Q-347 (E2E #3p, 3.0.69, 27B tensor split): the first chat request after an auto-compaction
# (12:32:50, 44,053 tokens: the 101,576-char system prompt + 81 tools + the summary) read 0 from
# the cache and prefilled for 150 s, although every chat request before it carried the same system
# prompt and tools byte for byte (the Q-342 captures: req6, req8 and the post-compaction request
# render one 40,361-token head under the 27B's own template, thinking off as the split renders a
# request carrying tools — the same render gives exactly the engine's 44,053 and 139,503). mlx_lm
# 0.31.3 snapshots a segment
# only past the tokens a request read from the cache (`_generate` pops the consumed segments), and
# it cuts a system segment only on a request that ends on a user message — so the head lived in
# ONE entry, the session's first request's (10:54:29–10:57:41: system 2 sequences, 1.48 GB = the
# head's 1.40 GB + a helper's 0.08), which mlx_lm's type-count eviction took at 10:59:33 (the
# oldest "system" entry once helpers' system segments outnumbered the rest: system 0.16 GB). From
# then on the head only lived inside longer, non-trimmable conversation entries; once compaction
# replaced the messages nothing matched. The post-compaction request re-made it (12:35:27: system
# 1 sequence, 1.40 GB) and helpers evicted it again within a minute (12:36:22: 0.08 GB).
#
# The stable head is what every request of the chat shares: the leading system messages and the
# tools, up to where mlx_lm's own probe (the system messages + an empty user turn) stops agreeing
# with the prompt — mlx_lm's own system-segment end, so a request ending on a user message is cut
# exactly where upstream cuts it. Every agent request (one naming its transient tail) now ends a
# segment there, so a request that reads less than the head from the cache leaves the entry, and
# the cache keeps it after the conversation prefix (`pop_keeping`, least precious last kept).


def head_probe(messages):
    """mlx_lm 0.31.3's probe for the end of a chat prompt's system block (server.py `_tokenize`):
    the leading system messages and an empty user turn, to render without a generation prompt.
    None when the conversation opens on no system message."""
    count = next((i for i, m in enumerate(messages) if m.get("role") != "system"), len(messages))
    if count == 0:
        return None
    return [*messages[:count], {"role": "user", "content": ""}]


def head_end(prompt, probe):
    """Where `prompt` stops agreeing with the rendered `probe` — as mlx_lm computes its system
    segment's end, 0 (no head) when one is a prefix of the other."""
    at = common_prefix(prompt, probe)
    return at if at < min(len(prompt), len(probe)) else 0


def cut_at_head(segments, segment_types, head):
    """mlx_lm's segments with one ending at `head`, typed "system" as mlx_lm types the system
    segment it cuts itself. Unchanged when a segment already ends there, or when the head is not
    strictly inside the prompt."""
    cut, types, start = [], [], 0
    for segment, kind in zip(segments, segment_types):
        end = start + len(segment)
        if start < head < end:
            cut += [segment[: head - start], segment[head - start :]]
            types += ["system", kind]
        else:
            cut.append(segment)
            types.append(kind)
        start = end
    return cut, types
