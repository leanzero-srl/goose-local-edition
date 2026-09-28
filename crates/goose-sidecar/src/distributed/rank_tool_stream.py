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
#                                    a value ends at the LAST "</parameter>" before the next
#                                    declared parameter's header (Q-372, below), else before the
#                                    call's end
#   a string-typed VALUE is itself, except the exact word "null" (any case) is None; any other type
#   is converted from the whole value — so only string values can stream before their close.
#
# Q-372 (2026-09-28): mlx_lm 0.31.3 ends a value at its FIRST `</parameter>` (`_parameter_regex`,
# non-greedy), so a written file that holds the text `</parameter>` — code or docs about this very
# format, a JS string `"</parameter>"` — arrived CUT there, with the call reported as a success:
# measured offline, `const close = "</parameter>";` in a `write` became content ending at
# `const close = "` and nothing said so (the streamer mirrored the parser, so the two agreed on the
# cut). The single engine reads the same wire positionally (Rapid-MLX `tool_call_scan.py`,
# omlx#2507): a value runs to the next SIBLING header — a `<parameter=NAME>` whose NAME the tool
# declares, with a `</parameter>` before it — and ends at the last `</parameter>` before that
# sibling (or before the call's end). `install_positional_parameters` puts that reading under
# mlx_lm's own `parse_tool_call`, and the streamer below reads the same rule, so a value that holds
# the close tag streams up to it and is finished when the sibling or the call's end places it.

import json  # noqa: E402
import re  # noqa: E402

PARAMETER_HEADER = re.compile(r"<parameter=([^>]*)>")
PARAMETER_CLOSE = "</parameter>"


def _trim_wrapping_newlines(value):
    if value.startswith("\n"):
        value = value[1:]
    if value.endswith("\n"):
        value = value[:-1]
    return value


def sibling_header(text, value_start, valid_names, headers=None):
    """The header that ends the value opened at `value_start`: the first `<parameter=NAME>` after
    it whose NAME is declared (any NAME when the tool declares none) with a `</parameter>` between
    the value's start and it. None while there is none."""
    for header in headers if headers is not None else PARAMETER_HEADER.finditer(text, value_start):
        if header.start() < value_start:
            continue
        if valid_names is not None and header.group(1) not in valid_names:
            continue
        if text.find(PARAMETER_CLOSE, value_start, header.start()) >= 0:
            return header
    return None


def parameters_by_position(text, valid_names):
    """`[(KEY, VALUE)]` of a function body, in the order written: each value runs to its sibling
    header (or the body's end) and ends at the last `</parameter>` before it; a value with no
    `</parameter>` at all is not read (mlx_lm's regex required the close too)."""
    headers = list(PARAMETER_HEADER.finditer(text))
    read = []
    i = 0
    while i < len(headers):
        sibling = sibling_header(text, headers[i].end(), valid_names, headers[i + 1:])
        end = sibling.start() if sibling is not None else len(text)
        cut = text.rfind(PARAMETER_CLOSE, headers[i].end(), end)
        if cut >= 0:
            read.append((headers[i].group(1), _trim_wrapping_newlines(text[headers[i].end():cut])))
        if sibling is None:
            break
        i = headers.index(sibling)
    return read


def install_positional_parameters(qwen3_coder):
    """mlx_lm's `_parse_xml_function_call`, reading its parameters by position (Q-372). The name,
    the schema lookup and every value's conversion stay mlx_lm's own functions, read from the
    module when a call is parsed (so Q-232's `_get_arguments_config` is the one used)."""

    def _parse_xml_function_call(function_call_str, tools):
        end_index = function_call_str.index(">")
        function_name = function_call_str[:end_index]
        param_config = qwen3_coder._get_arguments_config(function_name, tools)
        arguments = {}
        for name, value in parameters_by_position(
            function_call_str[end_index + 1:], set(param_config) or None
        ):
            arguments[name] = qwen3_coder._convert_param_value(value, name, param_config)
        return dict(name=function_name, arguments=arguments)

    qwen3_coder._parse_xml_function_call = _parse_xml_function_call


class ToolCallStream:
    """One tool call: the text between `<tool_call>` and `</tool_call>`, fed as it is generated.

    `convert(value, key, config)` is the parser's own value conversion and `config` the call's
    parameter schema (the parser's `_get_arguments_config`, read once the name is known)."""

    FUNCTION_OPEN = "<function="
    PARAM_OPEN = "<parameter="
    PARAM_CLOSE = "</parameter>"
    FUNCTION_CLOSE = "</function>"
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
        # Where the markup the streamer is waiting on starts: the call's first character, then the
        # end of the function header or of the last `</parameter>` (Q-146's `stray`).
        self._markup = 0
        # The open value's first `</parameter>` (Q-372: it may be the value's own text) and where
        # the search for its sibling header resumes.
        self._first_close = None
        self._header_scan = 0

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

    def position(self):
        """Where the streamer is reading, for a reader of the running request (Q-146): the phase
        (head: the function header; between: waiting for a parameter or `</function>`; key: a
        parameter's name; value: its value), the open parameter, whether its value streams, and the
        streamer's verdict when it stopped reading."""
        value = self._phase == "value"
        return {
            "name": self.name,
            "phase": self._phase,
            "parameter": self._key if value else None,
            "string_value": self._string if value else None,
            "broken": self.broken,
            "sent_chars": len(self.sent),
        }

    def typed_value_open(self):
        """The open parameter whose value is sent only when it closes (a non-string type), or None."""
        if self.broken is None and self._phase == "value" and not self._string:
            return self._key
        return None

    def stray(self):
        """Text where the streamer waits for the qwen3_coder frame (`<function=` at the head,
        `<parameter=` or `</function>` between parameters and after a value's `</parameter>`) that
        is not that frame — a call written another way (JSON inside `<tool_call>`, prose between
        parameters), of which the streamer reads, and so sends, nothing. After a `</parameter>`
        inside a value (Q-372) the same text may still be the value's own: the parser decides when
        the next declared header or the call's end places the value's last `</parameter>`. None
        while the text is the frame or a prefix of it."""
        if self.broken is not None:
            return None
        if self._phase == "value":
            if self._first_close is None:
                return None
            last = self.text.rfind(self.PARAM_CLOSE, self._first_close)
            waiting = self.text[last + len(self.PARAM_CLOSE):].lstrip()
            frames = (self.PARAM_OPEN, self.FUNCTION_CLOSE)
        elif self._phase == "head":
            waiting = self.text[self._markup :].lstrip()
            frames = (self.FUNCTION_OPEN,)
        elif self._phase == "between":
            waiting = self.text[self._markup :].lstrip()
            frames = (self.PARAM_OPEN, self.FUNCTION_CLOSE)
        else:
            return None
        if not waiting or any(f.startswith(waiting) or waiting.startswith(f) for f in frames):
            return None
        return waiting

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
        self._markup = self._cursor
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
        if self._first_close is None:
            close = self.text.find(self.PARAM_CLOSE, max(self._value_start, self._cursor))
            if close < 0:
                self._cursor = max(self._value_start, len(self.text) - len(self.PARAM_CLOSE) + 1)
                return self._string_increment(len(self.text) - self.HOLD)
            self._first_close = close
            self._header_scan = close + len(self.PARAM_CLOSE)
        sibling = self._sibling()
        if sibling is None:
            # Everything before the first `</parameter>` is the value's whatever follows, but its
            # last "\n" is markup if the value ends there.
            return self._string_increment(self._first_close - 1)
        end = sibling.start()
        cut = self.text.rfind(self.PARAM_CLOSE, self._value_start, end)
        value = _trim_wrapping_newlines(self.text[self._value_start:cut])
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
        self._cursor = end
        self._markup = cut + len(self.PARAM_CLOSE)
        self._first_close = None
        self._phase = "between"
        return None, fragment

    def _sibling(self):
        """The open value's sibling header (`sibling_header`), searched from where the last search
        stopped: every header after the first `</parameter>` has one before it."""
        valid = set(self._config) or None
        complete_end = self._header_scan
        for header in PARAMETER_HEADER.finditer(self.text, self._header_scan):
            if valid is None or header.group(1) in valid:
                return header
            complete_end = header.end()
        pending = self.text.rfind(self.PARAM_OPEN, complete_end)
        if pending < 0:
            pending = max(complete_end, len(self.text) - len(self.PARAM_OPEN) + 1)
        self._header_scan = pending
        return None

    def _string_increment(self, safe_end):
        """The certain part of an open string value, up to `safe_end`: nothing until more of it is
        certain than the word "null" (a string value that is exactly that word is None), then all
        of it but what may still be its trailing newline and close tag."""
        if not self._string:
            return None
        lead = 1 if self.text.startswith("\n", self._value_start) else 0
        certain = self.text[self._value_start + lead : safe_end] if safe_end > self._value_start else ""
        if len(certain) <= (self._value_sent or len(self.NULL)):
            return None
        piece = json.dumps(certain[self._value_sent:], ensure_ascii=False)[1:-1]
        prefix = "" if self._value_sent else self._key_prefix() + '"'
        self._value_sent = len(certain)
        return None, prefix + piece
