# goose distributed tensor rank: a tool call's arguments streamed while the model writes them (Q-141).
# Pure stdlib; concatenated after rank_state.py, before rank_wrapper.py (which feeds it).
#
# mlx_lm 0.31.3's handle_completion sends nothing while its state machine is in "tool"
# (server.py:1478, `gen.state != "tool"`): the whole call goes out in one frame when `</tool_call>`
# arrives. E2E #3c (2026-09-26, 3.0.51): one agent call wrote 12,556+ tokens over 18+ minutes at
# ~11.8 tok/s and goose's request log held ZERO chunks; the chat said only "Writing". Rapid-MLX (the
# single engine) and the fork's pipeline runner stream the same call as OpenAI `tool_calls` deltas:
# an open frame (index, id, name) and `function.arguments` fragments, which goose's decoder
# (formats/openai.rs) accumulates per index and shows as its forming line.
#
# The fragments are exact by construction. The streamer sends only what is already certain of the
# parser's own serialization, `json.dumps(arguments, ensure_ascii=False)` — the object's `{`, each
# parameter's key, a string value's characters as they arrive, a typed value once it closes — and
# when the call ends the tokenizer's own parser reads the whole text, exactly as mlx_lm's
# ToolCallFormatter would: if what was sent is a prefix of that serialization, the rest (the `}` at
# least) is sent and the call the client assembles is byte-identical to the one mlx_lm would have
# sent whole. If it is not (the parser refuses the text, or the text broke the streamer's reading),
# nothing more is sent: the client holds unterminated arguments and fails the call LOUDLY — goose
# answers it as a failed tool request the model sees — where mlx_lm dropped an unparseable call
# without a word (ToolCallFormatter's `continue`).
#
# The qwen3_coder XML only (mlx_lm/tool_parsers/qwen3_coder.py — the chat template of every qwen3_5
# checkpoint the tensor runner serves); its reading, mirrored:
#   <function=NAME>                  NAME = up to the first ">"
#   <parameter=KEY>\nVALUE\n</parameter>   one leading and one trailing "\n" are not the value;
#                                    a value ends at the FIRST "</parameter>" (non-greedy regex)
#   a string-typed VALUE is itself, except the exact word "null" (any case) is None; any other type
#   is converted from the whole value — so only string values can stream before their close.

import json  # noqa: E402


class ToolCallStream:
    """One tool call: the text between `<tool_call>` and `</tool_call>`, fed as it is generated.

    `convert(value, key, config)` is the parser's own value conversion and `config` the call's
    parameter schema (the parser's `_get_arguments_config`, read once the name is known)."""

    FUNCTION_OPEN = "<function="
    PARAM_OPEN = "<parameter="
    PARAM_CLOSE = "</parameter>"
    # A string value's last characters may still be its stripped trailing "\n" and the start of its
    # close tag: that many stay unsent until the close is seen.
    HOLD = len("\n</parameter>")
    # Converted by a string-typed parameter to itself, by every other type to something else or an
    # error (int/float/bool/literal_eval/json all refuse or change it).
    STRING_PROBE = "\x00goose-string-probe"
    NULL = "null"

    def __init__(self, convert, arguments_config):
        self._convert = convert
        self._arguments_config = arguments_config
        self.text = ""
        self.name = None
        self.sent = ""
        self.broken = None
        self._config = {}
        self._cursor = 0
        self._phase = "head"
        self._key = None
        self._value_start = 0
        self._string = False
        self._value_sent = 0
        self._keys = []

    def feed(self, piece, tools):
        """Append generated text; returns (name when the call just opened else None, fragment)."""
        self.text += piece
        opened = None
        fragments = []
        while self.broken is None:
            step = self._step(tools)
            if step is None:
                break
            name, fragment = step
            if name is not None:
                opened = name
            fragments.append(fragment)
        fragment = "".join(fragments)
        self.sent += fragment
        return opened, fragment

    def close(self, parse, tools):
        """The call ended. Returns (fragment, verdict): the remainder of the parser's serialization
        and None when what was sent is a prefix of it; (None, why) when it is not."""
        try:
            parsed = parse(self.text, tools)
        except Exception as refusal:  # the parser's verdict is the call's: said, never swallowed
            return None, f"the parser refused the call: {type(refusal).__name__}: {refusal}"
        if isinstance(parsed, list):
            if len(parsed) != 1:
                return None, f"the parser read {len(parsed)} calls from one tool_call block"
            parsed = parsed[0]
        if parsed.get("name") != self.name:
            return None, f"the parser named {parsed.get('name')!r}, the stream opened {self.name!r}"
        whole = json.dumps(parsed["arguments"], ensure_ascii=False)
        if not whole.startswith(self.sent):
            return None, self.broken or "what was streamed is not a prefix of the parsed arguments"
        rest = whole[len(self.sent):]
        self.sent = whole
        return rest, None

    def _step(self, tools):
        if self._phase == "head":
            return self._head(tools)
        if self._phase == "between":
            return self._between()
        if self._phase == "key":
            return self._read_key()
        return self._value()

    def _head(self, tools):
        opener = self.text.find(self.FUNCTION_OPEN)
        if opener < 0:
            return None
        start = opener + len(self.FUNCTION_OPEN)
        end = self.text.find(">", start)
        if end < 0:
            return None
        self.name = self.text[start:end]
        self._config = self._arguments_config(self.name, tools)
        self._cursor = end + 1
        self._phase = "between"
        return self.name, "{"

    def _between(self):
        opener = self.text.find(self.PARAM_OPEN, self._cursor)
        if opener < 0:
            self._cursor = max(self._cursor, len(self.text) - len(self.PARAM_OPEN) + 1)
            return None
        self._cursor = opener + len(self.PARAM_OPEN)
        self._phase = "key"
        return None, ""

    def _read_key(self):
        end = self.text.find(">", self._cursor)
        if end < 0:
            return None
        key = self.text[self._cursor:end]
        if self.PARAM_CLOSE[:-1] in key:
            self.broken = "a parameter header closed without its name's '>'"
            return None
        if key in self._keys:
            # The parser keeps the key's first place and its LAST value: what was sent may differ.
            self.broken = f"parameter {key!r} written twice"
            return None
        self._key = key
        self._value_start = end + 1
        self._value_sent = 0
        try:
            self._string = self._convert(self.STRING_PROBE, key, self._config) == self.STRING_PROBE
        except Exception:  # a refused probe is a typed parameter, the probe's only question
            self._string = False
        self._phase = "value"
        return None, ""

    def _key_prefix(self):
        separator = ", " if self._keys else ""
        return f"{separator}{json.dumps(self._key, ensure_ascii=False)}: "

    def _value(self):
        close = self.text.find(self.PARAM_CLOSE, max(self._value_start, self._cursor))
        if close < 0:
            self._cursor = max(self._value_start, len(self.text) - len(self.PARAM_CLOSE) + 1)
            return self._string_increment()
        value = self.text[self._value_start:close]
        if value.startswith("\n"):
            value = value[1:]
        if value.endswith("\n"):
            value = value[:-1]
        if self._string and self._value_sent:
            fragment = json.dumps(value[self._value_sent:], ensure_ascii=False)[1:-1] + '"'
        else:
            try:
                converted = self._convert(value, self._key, self._config)
            except Exception as refusal:  # the parser refuses the same value when the call ends
                self.broken = f"parameter {self._key!r}: {type(refusal).__name__}: {refusal}"
                return None
            fragment = self._key_prefix() + json.dumps(converted, ensure_ascii=False)
        self._keys.append(self._key)
        self._cursor = close + len(self.PARAM_CLOSE)
        self._phase = "between"
        return None, fragment

    def _string_increment(self):
        """The certain part of an open string value: nothing until more of it is certain than the
        word "null" (a string value that is exactly that word is None), then all of it but the
        HOLD characters that may still be its trailing newline and close tag."""
        if not self._string:
            return None
        lead = 1 if self.text.startswith("\n", self._value_start) else 0
        safe_end = len(self.text) - self.HOLD
        certain = self.text[self._value_start + lead : safe_end] if safe_end > self._value_start else ""
        if len(certain) <= (self._value_sent or len(self.NULL)):
            return None
        piece = json.dumps(certain[self._value_sent:], ensure_ascii=False)[1:-1]
        prefix = "" if self._value_sent else self._key_prefix() + '"'
        self._value_sent = len(certain)
        return None, prefix + piece
