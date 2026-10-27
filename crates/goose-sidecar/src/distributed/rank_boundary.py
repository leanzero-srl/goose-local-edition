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
# `KeptEntry` since `keep_conversation_prefix`; every conversation's, not only the newest one's,
# since `keep_every_conversation`: Q-502's `Conversations`). The chat's stable head — its system
# prompt and tools — is cut and kept the same way (Q-347, `cut_at_head`, on a spec that asks for
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


# Q-498 (E2E #3x, 2026-09-29, the 27B tensor split at 262,144 tokens, its 17,333,813,248 B KV
# plan): the owner used a second chat while #3x's ~200k-token chat worked. At 15:10:16 local the
# cache still held both chats' prefixes (user 3 sequences 9.30 GB, system 4 3.06 GB: #3x's 6.62 GB
# conversation prefix and 1.47 GB head, the second chat's 2.59 GB and 1.43 GB); then an 84-token
# side call joined the second chat's 77,683-token row, the two rows padded to 77,683 charged
# 11.54 GB, the cache was trimmed to the 5.79 GB left, and the kept entries were the NEWEST
# conversation's — the second chat's (15:10:28: 4.34 GB). #3x's next call read all 200,456 tokens
# again, ~11 minutes. Keeping both needs 23.66 GB of KV charge where the plan has 17.33, and the
# MacBook rank had no ~7 GB more to give (Q-498's measurement), so the engine SAYS it: rank 0
# records every entry the cache evicts, and a request that extends an evicted entry past what the
# cache supplied carries it on its /v1/status row — how long it was, and whether the conversation
# the cache kept at that eviction was this one or another. Recording only: nothing here changes
# what is evicted, so every rank still evicts alike.
import hashlib  # noqa: E402
import threading  # noqa: E402
from array import array  # noqa: E402
from collections import deque  # noqa: E402


def tokens_bytes(tokens):
    return array("q", tokens).tobytes()


def key_digest(tokens):
    return hashlib.blake2b(tokens_bytes(tokens), digest_size=16).digest()


class EvictionLog:
    """Rank 0's record of the prompt-cache entries evicted: each key's length and digest, the
    conversation prefix the cache kept at that moment (its length and digest, or None), the batch
    the room was made for, and when. At most `capacity` records, the oldest dropped first — the
    cache's own entry bound (`prompt_cache_entries`)."""

    def __init__(self, capacity):
        self.records = deque(maxlen=max(1, capacity))
        self.lock = threading.Lock()
        self.kept = (None, None)

    def kept_key(self, kept):
        """(length, digest) of the kept conversation prefix, hashed once per key list."""
        if kept is None:
            return None
        if self.kept[0] is not kept:
            self.kept = (kept, key_digest(kept))
        return len(kept), self.kept[1]

    def evicted(self, tokens, kept, rows, width, requests, at):
        """The cache evicted the entry keyed `tokens` while it kept `kept` (a key list, or None),
        making room for a batch of `rows` rows at `width` serving `requests`, at `at`."""
        record = {
            "tokens": len(tokens),
            "digest": key_digest(tokens),
            "kept": self.kept_key(kept),
            "rows": rows,
            "width": width,
            "requests": list(requests),
            "at": at,
        }
        with self.lock:
            self.records.append(record)

    def lost_prefix(self, prompt, cached, at):
        """What `prompt` lost to an eviction, looked up at `at`: the longest evicted entry whose
        key is a prefix of it and longer than the `cached` tokens the cache supplied — its length,
        whether the conversation the cache kept at that eviction is this prompt's own
        (`this_conversation`), another's (`another_conversation`) or none was kept (None), the
        batch the room was made for and how long ago — or None when nothing it extends was
        evicted. One pass over the prompt, hashed at each length a record names."""
        with self.lock:
            records = [r for r in self.records if cached < r["tokens"] <= len(prompt)]
        if not records:
            return None
        lengths = {r["tokens"] for r in records}
        lengths |= {r["kept"][0] for r in records if r["kept"] and r["kept"][0] <= len(prompt)}
        digests, running, done = {}, hashlib.blake2b(digest_size=16), 0
        for length in sorted(lengths):
            running.update(tokens_bytes(prompt[done:length]))
            done = length
            digests[length] = running.copy().digest()
        lost = [r for r in records if digests[r["tokens"]] == r["digest"]]
        if not lost:
            return None
        record = max(lost, key=lambda r: (r["tokens"], r["at"]))
        kept = record["kept"]
        if kept is None:
            keeping = None
        elif digests.get(kept[0]) == kept[1]:
            keeping = "this_conversation"
        else:
            keeping = "another_conversation"
        return {
            "tokens": record["tokens"],
            "while_keeping": keeping,
            "rows": record["rows"],
            "width": record["width"],
            "requests": record["requests"],
            "ago_s": round(at - record["at"], 3),
        }


# Q-502 (E2E #3x, the same 15:10:16 sequence as Q-498 above): the kept entries were ONE
# conversation's — the newest cut — so the second chat's cut moved the protection off #3x, and the
# side call's admission left room for the second chat alone. Replayed through the real mlx_lm
# 0.31.3 LRUPromptCache, holding that 84-token side call until the second chat's 77,683-token row
# finished keeps #3x's prefix at the plan's 17,333,813,248 B, and #3x's next call reads 199,798
# tokens from the cache instead of 0 — with no new memory. So, on a spec that asks for it
# (`keep_every_conversation`): every conversation's stable prefix and head are kept while any stale
# end or other entry is left to evict (`Conversations`, `keep_conversations`), and rank 0 admits a
# request into a live batch only while the batch leaves room for all of them (rank_prefill.py
# `admits`, `other_kept_bytes`). A conversation is named by its tokens alone — a request whose
# prompt extends a conversation's kept prefix continues it — so every rank tracks the same
# conversations over the same requests and inserts, and every rank evicts alike.


class Conversation:
    """One conversation the prompt cache keeps: its stable prefix (a `KeptEntry` each of its
    requests cuts anew — the newer key, once inserted, replaces the older, which is then a stale
    entry like any other), the key of its stable head (the entry lives in `Conversations.heads`,
    shared by every conversation whose head is the same tokens), the cut that last named it (a
    count, the same on every rank), and the client's own name for it (Q-508: goose's session id;
    None = a client that names none, told apart by its tokens)."""

    def __init__(self, used, name=None):
        self.prefix = KeptEntry()
        self.head = None
        self.used = used
        self.name = name


# Q-508: told apart by tokens alone, a chat that COMPACTED left its pre-compaction prefix behind as
# a conversation of its own — nothing in the post-compaction prompt (the same head, a summary, no
# old message) extends it — kept and reserved for as if another chat would read it again. At E2E
# #3w's sizes (rank0 ...1790649911215: the head 1,453,850,624 B = 42,019 tokens, the last
# pre-compaction prefix 6,946,553,856 B = 209,643 tokens at 08:12:56Z; the compacted chat's calls
# 49,189 → 77,686 tokens) every side call joining the compacted chat's row waits for that row (the
# orphan's 6.95 GB counted as another conversation's; by the chat's own entries alone it joins up to
# 87,396 tokens), and once a lone row needs the room (134,265 tokens) the shared head — kept after
# every prefix — goes before the orphan (156,449). goose names its chat on every request (the
# `agent-session-id` header, `session_context::session_id_request_builder` on the omlx provider):
# a request carrying that name continues ITS conversation whatever its tokens, so a compacted
# chat's older prefix is the same conversation's stale entry — evicted before anything kept — the
# moment its request is cut. Every rank decides alike: rank 0's handler reads the header onto the
# request it shares (pickled whole, as the transient tail rides), and every rank's `_tokenize`
# passes it here. A request naming no conversation (another client) is told apart by its tokens
# among the unnamed ones, and the rank log says so once per such client
# (GOOSE_RANK_CONVERSATION_UNNAMED).


class Conversations:
    """Every conversation's kept entries on a rank (Q-502), named by the client where it names
    them (Q-508)."""

    def __init__(self):
        self.all = []
        self.heads = {}
        self.cuts = 0

    def cut(self, prompt, boundary, head, name=None):
        """A request naming its transient tail cut its stable prefix at `boundary` and its head at
        `head` (0 = none). A request carrying the client's `name` for its conversation continues
        the conversation of that name — and a held prefix its prompt does not extend (a
        compaction, a rewritten history) is that conversation's past, no longer kept — else opens
        one of that name. An unnamed request continues the unnamed conversation whose kept prefix
        its prompt extends (the longest), else an unnamed one holding no prefix — none held, none
        awaited — whose head it shares (its prefix went and the chat goes on), else it opens one.
        Only a conversation's newest cut is awaited: an earlier request of it that never inserted
        its prefix never will."""
        self.cuts += 1
        head_key = prefix_key(prompt[:head]) if head else None
        if name is not None:
            conversation = next((c for c in self.all if c.name == name), None)
            if conversation is not None and not extends(prompt, conversation.prefix.tokens):
                conversation.prefix.forget()
        else:
            conversation = self.extended_by(prompt)
            if conversation is None and head_key is not None:
                conversation = next(
                    (
                        c
                        for c in self.all
                        if c.name is None
                        and c.head == head_key
                        and c.prefix.tokens is None
                        and not c.prefix.cut_keys
                    ),
                    None,
                )
        if conversation is None:
            conversation = Conversation(self.cuts, name)
            self.all.append(conversation)
        conversation.used = self.cuts
        if boundary:
            conversation.prefix.cut_keys.clear()
            conversation.prefix.cut(prompt[:boundary])
        if head_key is not None:
            conversation.head = head_key
            entry = self.heads.setdefault(head_key, KeptEntry())
            entry.cut_keys[head_key[0]] = head_key[1]
        self.drop_empty(conversation)
        return conversation

    def extended_by(self, prompt):
        """The unnamed conversation whose held prefix `prompt` extends (the longest), or None."""
        found = None
        for c in self.all:
            tokens = c.prefix.tokens
            if c.name is not None or tokens is None:
                continue
            if found is not None and len(tokens) <= len(found.prefix.tokens):
                continue
            if extends(prompt, tokens):
                found = c
        return found

    def drop_empty(self, current):
        """Forget every conversation but `current` that holds nothing and awaits nothing, and every
        head no conversation names."""
        self.all = [
            c
            for c in self.all
            if c is current
            or c.prefix.tokens is not None
            or c.prefix.cut_keys
            or (c.head is not None and self.heads[c.head].tokens is not None)
        ]
        named = {c.head for c in self.all}
        self.heads = {key: entry for key, entry in self.heads.items() if key in named}

    def inserted(self, tokens, nbytes):
        for c in self.all:
            c.prefix.inserted(tokens, nbytes)
        for entry in self.heads.values():
            entry.inserted(tokens, nbytes)

    def recent(self):
        return sorted(self.all, key=lambda c: c.used, reverse=True)

    def kept(self):
        """Every conversation's kept entries, most precious first: the stable prefixes, the most
        recently cut conversation's first, then the heads in the same order. A head is the part
        of its conversation's prefix every request of the chat shares, read on its own only once
        that prefix is gone (a compaction), so while room is short every conversation keeps what
        its next request extends before any keeps its head."""
        recent = self.recent()
        entries = [c.prefix for c in recent]
        for c in recent:
            entry = self.heads.get(c.head)
            if entry is not None and all(entry is not other for other in entries):
                entries.append(entry)
        return entries

    def newest_prefix(self):
        """The held prefix of the most recently cut conversation that holds one, or None."""
        held = [c for c in self.recent() if c.prefix.tokens is not None]
        return held[0].prefix.tokens if held else None

    def reserve(self):
        """What a batch leaves the cache (rank_prefill.py `admits`): the most recently cut
        conversation's held prefix bytes and head bytes, the bytes of every other kept entry held
        (each entry once), and how many conversations hold anything."""
        recent = self.recent()
        newest = recent[0] if recent else None
        own = [newest.prefix, self.heads.get(newest.head)] if newest is not None else [None, None]
        prefix, head = (e.nbytes if e is not None and e.tokens is not None else 0 for e in own)
        counted = {id(e.tokens) for e in own if e is not None and e.tokens is not None}
        other = 0
        for entry in self.kept():
            if entry.tokens is not None and id(entry.tokens) not in counted:
                counted.add(id(entry.tokens))
                other += entry.nbytes
        holding = sum(
            1
            for c in recent
            if c.prefix.tokens is not None
            or (c.head in self.heads and self.heads[c.head].tokens is not None)
        )
        return prefix, head, other, holding


def extends(prompt, tokens):
    """Whether `prompt` starts with the held key `tokens` (None = nothing held: False)."""
    return tokens is not None and len(tokens) <= len(prompt) and prompt[: len(tokens)] == tokens


def keep_conversations(cache_order, conversations):
    """Install `pop_keeping` of every conversation's kept entries (`Conversations.kept`, read at
    each eviction) on mlx_lm's `LRUPromptCache.CacheOrder`."""
    upstream_pop = cache_order.pop

    def pop(self):
        return pop_keeping(self, upstream_pop, conversations.kept())

    cache_order.pop = pop
