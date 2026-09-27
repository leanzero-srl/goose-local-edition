# goose distributed tensor rank: the prompt cache's nearest-entry search, in time linear in the
# trie nodes it visits (Q-162). Pure stdlib; concatenated after rank_state.py, before
# rank_wrapper.py, whose prompt cache searches through it.
#
# E2E #3e (2026-09-27 07:27): goose's compaction call (259,408 tokens on a 262,144 window) sat on
# rank 0 in mlx_lm 0.31.3's PromptTrie.search (models/cache.py:1612) for 22 s with no step and no
# GPU time, and goosed's hang rule stopped the split. The search's "longer" walk is a depth-first
# search below the node where the prompt leaves the trie, and every push copies the whole path so
# far (`extra + [tok]`): a walk down one D-token branch copies D²/2 list slots. The cache then held
# two ~260k-token entries; the prompt left their shared path within its first tokens, so the walk
# went down a ~260k-token branch — 3.4·10^10 slot copies.
#
# `nearest_prompt` is that search step for step — the same walk, the same visiting order, the same
# pruning, the same tie among equally short entries — with the path kept as a linked list of
# (token, parent) cells, so each push is O(1) and the entry's tokens are unwound once at the end.
# Every rank runs its own cache over the same requests, so the result must be upstream's exactly:
# a rank whose cache reused a different prefix than its peers' would run a different number of
# prefill steps and pair its collectives with the wrong ones.

CACHE_LOOKUP = "cache_lookup"


def unwind(path):
    """The tokens of a walk's path: cells (token, parent) for a step out of a branching node, and
    (anchor, steps, parent) for a run down `steps` single-child nodes from `anchor`, re-walked here
    once for the one path that won instead of being recorded token by token for every path."""
    pieces = []
    while path is not None:
        if len(path) == 2:
            token, path = path
            pieces.append((token,))
        else:
            node, steps, path = path
            run = []
            for _ in range(steps):
                (token,) = node
                run.append(token)
                node = node[token]
            pieces.append(run)
    pieces.reverse()
    return [token for piece in pieces for token in piece]


def nearest_prompt(trie, model, tokens):
    """mlx_lm 0.31.3 `PromptTrie.search(model, tokens)` over the trie's root dict (`PromptTrie._trie`),
    as the tuple (model, exact, shorter, longer, common_prefix) of its `PromptTrieResult`."""
    if model not in trie:
        return model, None, None, None, 0

    current = trie[model]

    if not tokens and "__value__" in current:
        return model, [], None, None, 0

    last_index = -1
    index = 0
    while index < len(tokens) and tokens[index] in current:
        current = current[tokens[index]]
        if "__value__" in current:
            last_index = index
        index += 1

    if last_index == len(tokens) - 1 >= 0:
        return model, tokens, None, None, 0

    shorter = None
    if last_index > 0:
        shorter = tokens[: last_index + 1]

    longer = None
    common_prefix = index
    if index > 0:
        best_depth = None
        best_path = None
        stack = [(current, 0, None)]
        while stack:
            node, depth, path = stack.pop()
            # A node with one child and no entry: upstream pushes that child and pops it next, so
            # descending in place is the same walk without the stack round trip.
            anchor, anchor_depth = node, depth
            while len(node) == 1 and (best_depth is None or depth < best_depth):
                (token,) = node
                if token == "__value__":
                    break
                node = node[token]
                depth += 1
            if depth > anchor_depth:
                path = (anchor, depth - anchor_depth, path)
            if "__value__" in node:
                if best_depth is None or depth < best_depth:
                    best_depth, best_path = depth, path
            elif best_depth is None or depth < best_depth:
                for token in node:
                    stack.append((node[token], depth + 1, (token, path)))
        # Upstream's `tokens[:index] + best` with no entry below: the same TypeError.
        longer = tokens[:index] + (None if best_depth is None else unwind(best_path))
    return model, None, shorter, longer, common_prefix
