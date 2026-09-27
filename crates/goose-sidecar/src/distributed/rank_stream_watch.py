# goose distributed tensor rank: what a streamed chat answer's client has, and has not, been sent
# (Q-146). Pure stdlib; concatenated after rank_tool_stream.py, before rank_wrapper.py (which feeds
# it and serves its report on rank 0's /v1/status).
#
# E2E #3d turn 0 (2026-09-26, 3.0.52): goose's first agent call (prompt 39,996 tokens, 80 tools)
# generated 9,945+ tokens over 17+ minutes while the socket carried nothing, and nobody — goose, the
# user, the quality loop — could read a word of what the model was writing; two clean probes at the
# same time streamed fine through the Q-141 relay. A request the handler withholds text from is now
# visible while it runs and after it ends:
# - rank 0's /v1/status carries, per streamed chat request, `stream`: the output parser's state, the
#   Q-141 streamer's reading position and verdict, the characters generated / sent / generated since
#   the last frame that carried content, the withholding mode, and the last words written;
# - one `GOOSE_RANK_WITHHELD` line when the answer enters a mode that withholds text and one when it
#   leaves it, with the characters withheld and the words — so the rank's durable log keeps them.

# policy: the reader's window — gate 7's "read the last 2000-4000 characters" (Mihai, 2026-08-30),
# its smaller end: what one poll of /v1/status or one withheld line shows a person or the quality
# loop of the words being written. It bounds a report only; nothing about the generation reads it.
READER_TAIL_CHARS = 2000

# policy: "mostly" — the share of an answer's text outside its calls that one span, written back to
# back, must cover before the engine ends the answer (Q-161). E2E #3f turn 0: after one `write`
# call the text outside it was 3,251 chars of `!\n</parameter>\n</function>\n` and then
# `!\n</function>\n` over and over, never another call. A share of the answer's own text: an answer
# that says a line twice in passing is nowhere near it; one that has become the repeat is.
CYCLE_SHARE = 0.5


def verbatim_cycle(text):
    """(unit, copies) when `text` ends in one span written back to back at least twice — a span
    holding a line break and something besides whitespace — whose copies cover more than
    CYCLE_SHARE of `text`; else None. The shortest such span."""
    backwards = text[::-1]
    n = len(backwards)
    # z[p]: how far `text` read backwards from its end agrees with itself read from p further back
    # (the Z-function of the reversed text) — the tail written with period p runs z[p] + p chars.
    z = [0] * n
    left = right = 0
    for i in range(1, n):
        if i < right:
            z[i] = min(right - i, z[i - left])
        while i + z[i] < n and backwards[z[i]] == backwards[i + z[i]]:
            z[i] += 1
        if i + z[i] > right:
            left, right = i, i + z[i]
    for period in range(1, n // 2 + 1):
        copies = (z[period] + period) // period
        if copies < 2 or copies * period <= CYCLE_SHARE * n:
            continue
        unit = text[n - period :]
        if "\n" in unit and unit.strip():
            return unit, copies
    return None


def frame_chars(frame):
    """The content characters one chat.completion frame carries to the client: its text, its
    reasoning, and each tool call's name and argument text."""
    chars = 0
    for choice in frame.get("choices", []):
        delta = choice.get("delta") or choice.get("message") or {}
        chars += len(delta.get("content") or "") + len(delta.get("reasoning") or "")
        for call in delta.get("tool_calls") or []:
            function = call.get("function") or {}
            chars += len(function.get("name") or "") + len(function.get("arguments") or "")
    return chars


class StreamWatch:
    """One streamed chat request, as its handler receives it. `take` sees each generated piece
    before the handler does, `sent` each frame the handler builds, and `settle` runs once the
    handler (and the Q-141 relay) has done everything it does with a piece. `say(tag, payload)`
    prints a line (the wrapper's `emit`)."""

    def __init__(self, say):
        self._say = say
        self.request_id = None
        self.sequences = {}
        self.state = None
        self.generated_chars = 0
        self.sent_chars = 0
        self.since_sent_chars = 0
        self.content_frames = 0
        self.call_chars = 0
        self.tail = ""
        # The Q-141 streamer reading the call being written (set by the relay), or why none is.
        self.streamer = None
        self.unstreamed = None
        # Why the relay holds the call being written (Q-161: so far it is word for word an earlier
        # call of this answer), else None.
        self.holding = None
        self.episode = None
        # Why the engine ended this answer itself (Q-161: a call written again word for word, or
        # the text outside the calls become one span written over and over), else None.
        self.stop = None

    def take(self, gen):
        # A matched control sequence (`<tool_call>`, `</tool_call>`, an end of turn) reaches the
        # handler with its text emptied (mlx_lm's _process_control_tokens); the tail shows the
        # sequence the model wrote, the counts only what the handler could send.
        match = self.sequences.get(tuple(gen.match)) if gen.match is not None else None
        if gen.state == "tool" and self.state != "tool":
            self.call_chars = 0
        self.state = "stopped" if gen.state is None else gen.state
        self.tail = (self.tail + gen.text + (match or ""))[-READER_TAIL_CHARS:]
        chars = len(gen.text)
        self.generated_chars += chars
        self.since_sent_chars += chars
        if gen.state == "tool":
            self.call_chars += chars
        if self.episode is not None:
            self.episode["withheld_chars"] += chars

    def counting(self, build):
        """The handler's `generate_response`, counting the content of every frame it builds (each
        one is written the moment it is built)."""

        def counted(*args, **kwargs):
            frame = build(*args, **kwargs)
            chars = frame_chars(frame)
            if chars:
                self.sent_chars += chars
                self.since_sent_chars = 0
                self.content_frames += 1
            return frame

        return counted

    def withholding(self):
        """(mode, reason) while the answer is in a state that withholds its text, else None. The
        normal and reasoning states are sent piece by piece by mlx_lm's own handler."""
        if self.state != "tool":
            return None
        if self.streamer is None:
            return (
                "tool_not_streamed",
                self.unstreamed or "no tool-call streamer is attached to this answer",
            )
        if self.holding is not None:
            return ("tool_repeat_held", self.holding)
        if self.streamer.broken is not None:
            return (
                "tool_broken",
                f"the streamer stopped reading the call: {self.streamer.broken}; "
                "nothing more of it is sent",
            )
        if self.streamer.stray() is not None:
            return (
                "tool_unread",
                f"the call's text at its {self.streamer.position()['phase']} is not the "
                "qwen3_coder frame (<function=NAME>, <parameter=KEY>), so the streamer sends none of it",
            )
        typed = self.streamer.typed_value_open()
        if typed is not None:
            return (
                "tool_typed_value",
                f"parameter {typed!r} is not a string: its value is sent whole when "
                "</parameter> closes it",
            )
        return None

    def settle(self):
        mode = self.withholding()
        current = None
        if self.episode is not None:
            current = (self.episode["mode"], self.episode["reason"])
        if mode == current:
            return
        if self.episode is not None:
            self._leave("left")
        if mode is not None:
            self.episode = {"mode": mode[0], "reason": mode[1], "withheld_chars": self.since_sent_chars}
            self._say(
                "RANK_WITHHELD",
                {
                    "request_id": self.request_id,
                    "event": "enter",
                    "mode": mode[0],
                    "reason": mode[1],
                    "generated_chars": self.generated_chars,
                    "sent_chars": self.sent_chars,
                    "since_sent_chars": self.since_sent_chars,
                },
            )

    def end(self):
        self.settle()
        if self.episode is not None:
            self._leave("request ended")

    def _leave(self, how):
        episode, self.episode = self.episode, None
        self._say(
            "RANK_WITHHELD",
            {
                "request_id": self.request_id,
                "event": "leave",
                "how": how,
                "mode": episode["mode"],
                "reason": episode["reason"],
                "withheld_chars": episode["withheld_chars"],
                "since_sent_chars": self.since_sent_chars,
                "generated_chars": self.generated_chars,
                "sent_chars": self.sent_chars,
                "tail": self.tail,
            },
        )

    def report(self):
        """The `stream` block of the request's /v1/status row."""
        call = None
        if self.state == "tool":
            call = {"streamed": self.streamer is not None, "call_chars": self.call_chars}
            if self.streamer is not None:
                call.update(self.streamer.position())
            else:
                call["why_not_streamed"] = self.unstreamed
        episode = self.episode
        return {
            "parser_state": self.state,
            "generated_chars": self.generated_chars,
            "sent_chars": self.sent_chars,
            "since_sent_chars": self.since_sent_chars,
            "content_frames": self.content_frames,
            "withholding": None if episode is None else dict(episode),
            "tool_call": call,
            "stop": self.stop,
            "tail": self.tail,
            "tail_window_chars": READER_TAIL_CHARS,
        }
