# goose distributed tensor rank: a Qwen3-Coder XML tool call held to its template's skeleton where
# the template admits no free text (Q-161). Concatenated after rank_stream_watch.py, before
# rank_wrapper.py, which installs it on a launch whose spec asks for it (`xml_skeleton_guard`);
# importable on its own beside a real mlx (launch.rs's tests run it there).
#
# The single engine has held its decoder to this skeleton since Rapid-MLX v0.14.3-lz.7 (goose Q-85,
# `rapid_mlx/xml_tool_close_guard.py`); the split's mlx_lm 0.31.3 did not, and the same checkpoint
# leaves the format exactly where the turn should end. Measured on the split (2026-09-27, 8091,
# Qwen3.8-27B-Atlassian-Q8, E2E #3e's turn-0 request, the model's next-token log-probabilities
# after the template's own `</tool_call>`): one call written — `!` −0.00 (p≈1), `\n` −4.0,
# `<|im_end|>` −7.1; after the write + mkdir pair — `!` −0.13, `<|im_end|>` −2.6, `\n` −2.9. What
# follows the `!` is a runaway: a live continuation from that pair wrote
# `</tool_call>! 2>/dev/null\necho "done"\n</parameter>\n</function>\n</tool_call>!\n</parameter>\n!`
# and then `</parameter>\n!` for 3,000 tokens without an end token; a plain replay of the turn left
# the content parameter as `</parameter>\n! 2026-09-24 kickoff, Harbourline Jira DC→Cloud` and
# repeated that line to the token limit inside a call that never closed. Masking what the
# template does not allow moves that probability to the continuations it does — after the pair,
# the end of the turn (−2.6) outweighs another call (−2.9).
#
# The wire (the chat template) renders a call as
#   <tool_call>\n<function=NAME>\n<parameter=KEY>\nVALUE\n</parameter>\n...</function>\n</tool_call>
# and a turn that calls tools ends right after its last `</tool_call>` (another call is joined with
# `\n`). So at four positions nothing but the skeleton may come next:
#   - after a value's close (`</parameter>` at the start of a line, inside a call):
#     `\n<parameter` or `\n</function>`;
#   - after the function's close (`</function>` at the start of a line, inside a call):
#     `\n</tool_call>`;
#   - after `<tool_call>`: `\n<function`;
#   - after `</tool_call>`: `\n<tool_call>` or the end of the turn — and after that newline the end
#     of the turn is still allowed, so the guard never turns "wanted to stop" into another call.
# At those positions every other token is masked; nothing is rewritten after the fact and nothing
# is dropped: the model chooses among the legal continuations itself. Inside an open `<think>` the
# guard is inert. Every id is the tokenizer's own encoding of the template text, none assumed; a
# tokenizer whose template is not this wire (or splits a marker differently in context) gets no
# guard, and the wrapper says so.
#
# The processor is a pure function of the token history mlx_lm hands it (the row's whole context),
# so every rank computes the same mask from the same shared tokens and samples alike — which is
# why only a launch whose every rank runs it asks for it (launch.rs `mlxLmServerSkeletonGuard`).
from dataclasses import dataclass, field  # noqa: E402

import mlx.core as mx  # noqa: E402

XML_CALL_OPEN = "<tool_call>"
XML_CALL_CLOSE = "</tool_call>"
XML_THINK_OPEN = "<think>"
XML_THINK_CLOSE = "</think>"
XML_TEMPLATE_MARKERS = (
    XML_CALL_OPEN,
    XML_CALL_CLOSE,
    "<function=",
    "</function>",
    "<parameter=",
    "</parameter>",
)


class GuardUnarmed(Exception):
    """Why this tokenizer gets no skeleton guard (said by the wrapper, never swallowed)."""


@dataclass(frozen=True)
class SkeletonRule:
    """At `window` (the last tokens emitted), only `allowed` may come next. `inside_call` rules arm
    only while a `<tool_call>` is open (their window is ordinary text prose may also hold);
    `after_newline` rules also need the token just before `window` to end with a newline."""

    window: tuple
    allowed: frozenset
    inside_call: bool
    after_newline: bool = False


@dataclass
class SkeletonSpec:
    rules: tuple
    open_id: int
    close_id: int
    think_ids: tuple
    newline_ids: frozenset
    _masks: dict = field(default_factory=dict, repr=False)
    _newline_masks: dict = field(default_factory=dict, repr=False)

    def newline_mask(self, vocab_size):
        """`(vocab_size,)` bool: the ids whose text ends with a newline."""
        mask = self._newline_masks.get(vocab_size)
        if mask is None:
            ids = sorted(i for i in self.newline_ids if i < vocab_size)
            mask = mx.zeros((vocab_size,), dtype=mx.bool_)
            mask[mx.array(ids)] = True
            mx.eval(mask)
            self._newline_masks[vocab_size] = mask
        return mask

    def allowed_masks(self, vocab_size):
        """`(len(rules), vocab_size)` bool rows, built once per vocab width."""
        masks = self._masks.get(vocab_size)
        if masks is None:
            rows = []
            for rule in self.rules:
                row = mx.zeros((vocab_size,), dtype=mx.bool_)
                row[mx.array(sorted(rule.allowed))] = True
                rows.append(row)
            masks = mx.stack(rows)
            mx.eval(masks)
            self._masks[vocab_size] = masks
        return masks

    def report(self):
        return [[list(rule.window), sorted(rule.allowed)] for rule in self.rules]


def _template_text(tokenizer):
    template = getattr(tokenizer, "chat_template", None)
    if isinstance(template, dict):
        return "\n".join(t for t in template.values() if isinstance(t, str))
    return template if isinstance(template, str) else ""


def _encode(tokenizer, text):
    return tuple(int(i) for i in tokenizer.encode(text, add_special_tokens=False))


def _single_token_id(tokenizer, text):
    ids = _encode(tokenizer, text)
    if len(ids) != 1 or tokenizer.decode(list(ids)) != text:
        return None
    return ids[0]


def _rules_for(trigger, continuations, inside_call, after_newline=False):
    """One rule per prefix of the continuations: their trie walked token by token."""
    trie = {}
    for tail in continuations:
        for depth in range(len(tail)):
            trie.setdefault(tail[:depth], set()).add(tail[depth])
    return [
        SkeletonRule(trigger + prefix, frozenset(allowed), inside_call, after_newline)
        for prefix, allowed in sorted(trie.items(), key=lambda item: len(item[0]))
    ]


def _newline_ids(tokenizer):
    """Every id whose decoded text ends with a newline."""
    try:
        size = len(tokenizer)
    except TypeError:
        # mlx_lm's TokenizerWrapper forwards attributes to the HF tokenizer, not `len()`.
        vocab = tokenizer.get_vocab()
        size = max(vocab.values()) + 1
    ids = [[i] for i in range(size)]
    batch_decode = getattr(tokenizer, "batch_decode", None)
    texts = batch_decode(ids) if callable(batch_decode) else [tokenizer.decode(one) for one in ids]
    return frozenset(i for i, text in enumerate(texts) if text.endswith("\n"))


def _continuations(tokenizer, trigger_text, texts):
    """`trigger_text` and each continuation, as the tokenizer splits them in context."""
    trigger = _encode(tokenizer, trigger_text)
    tails = []
    for text in texts:
        full = _encode(tokenizer, trigger_text + text)
        # The trigger must tokenize the same whatever follows it, or the window would not be where
        # the model's tokens put it.
        if full[: len(trigger)] != trigger or len(full) == len(trigger):
            raise GuardUnarmed(
                f"{trigger_text + text!r} does not tokenize as {trigger_text!r} + its continuation"
            )
        tails.append(full[len(trigger):])
    return trigger, tails


def skeleton_spec(tokenizer, end_ids):
    """The skeleton rules for `tokenizer`; raises GuardUnarmed naming why its wire is not this one.
    `end_ids` are the ids that end a turn (mlx_lm's `eos_token_ids`)."""
    end_ids = frozenset(int(i) for i in end_ids)
    if not end_ids:
        raise GuardUnarmed("the tokenizer names no end-of-turn id")
    template = _template_text(tokenizer)
    missing = [marker for marker in XML_TEMPLATE_MARKERS if marker not in template]
    if missing:
        raise GuardUnarmed(f"the chat template does not render the XML tool call (no {missing})")
    open_id = _single_token_id(tokenizer, XML_CALL_OPEN)
    close_id = _single_token_id(tokenizer, XML_CALL_CLOSE)
    if open_id is None or close_id is None or open_id == close_id:
        raise GuardUnarmed("the tokenizer does not carry <tool_call> and </tool_call> as single tokens")
    think_open = _single_token_id(tokenizer, XML_THINK_OPEN)
    think_close = _single_token_id(tokenizer, XML_THINK_CLOSE)
    think_ids = (think_open, think_close) if think_open is not None and think_close is not None else None
    newline_ids = _newline_ids(tokenizer)
    if not newline_ids:
        raise GuardUnarmed("no id of this tokenizer decodes to text ending in a newline")
    after_close = _continuations(tokenizer, "</parameter>", ("\n<parameter", "\n</function>"))
    after_function = _continuations(tokenizer, "</function>", ("\n" + XML_CALL_CLOSE,))
    after_open = _continuations(tokenizer, XML_CALL_OPEN, ("\n<function",))
    next_call = _continuations(tokenizer, XML_CALL_CLOSE, ("\n" + XML_CALL_OPEN, "\n"))
    rules = []
    rules += _rules_for(*after_close, inside_call=True, after_newline=True)
    rules += _rules_for(*after_function, inside_call=True, after_newline=True)
    rules += _rules_for(*after_open, inside_call=False)
    another_call, newline = next_call[1]
    rules += _rules_for(
        next_call[0],
        [another_call]
        + [(end_id,) for end_id in sorted(end_ids)]
        + [newline + (end_id,) for end_id in sorted(end_ids)],
        inside_call=False,
    )
    return SkeletonSpec(
        rules=tuple(rules),
        open_id=open_id,
        close_id=close_id,
        think_ids=think_ids,
        newline_ids=newline_ids,
    )


class XmlSkeletonGuard:
    """mlx_lm logits processor `(tokens, logits) -> logits` enforcing a SkeletonSpec on one row."""

    def __init__(self, spec):
        self.spec = spec
        self._windows = [mx.array(rule.window, dtype=mx.int32) for rule in spec.rules]
        self._inside_call = mx.array([rule.inside_call for rule in spec.rules])

    @staticmethod
    def _last(history, positions, token_id):
        return mx.max(mx.where(history == token_id, positions, -1))

    def __call__(self, tokens, logits):
        history = tokens.reshape(-1)
        n = history.shape[0]
        active = [row for row, window in enumerate(self._windows) if window.shape[0] <= n]
        if not active:
            return logits
        tail = history.astype(mx.int32)
        matched = mx.stack(
            [mx.all(tail[-self._windows[row].shape[0]:] == self._windows[row]) for row in active]
        )
        positions = mx.arange(n)
        call_open = self._last(history, positions, self.spec.open_id) > self._last(
            history, positions, self.spec.close_id
        )
        rows = mx.array(active)
        newline = self.spec.newline_mask(logits.shape[-1])
        after_newline = []
        for row in active:
            width = self._windows[row].shape[0]
            if not self.spec.rules[row].after_newline:
                after_newline.append(mx.array(True))
            elif n > width:
                before = mx.minimum(tail[n - width - 1], newline.shape[0] - 1)
                after_newline.append(newline[before])
            else:
                after_newline.append(mx.array(False))
        armed = matched & mx.stack(after_newline) & (call_open | ~self._inside_call[rows])
        if self.spec.think_ids is not None:
            think_open, think_close = self.spec.think_ids
            thinking = self._last(history, positions, think_open) > self._last(
                history, positions, think_close
            )
            armed = armed & ~thinking
        masks = self.spec.allowed_masks(logits.shape[-1])[rows]
        allowed = mx.any(armed[:, None] & masks, axis=0)
        return mx.where(mx.any(armed), mx.where(allowed, logits, -mx.inf), logits)
