# goose distributed tensor rank: the request fields rank 0 refuses before any rank sees the request
# (Q-177). Pure stdlib; concatenated after rank_thinking.py, before rank_wrapper.py.
#
# mlx_lm 0.31.3's handler raises a plain ValueError from validate_model_parameters (`top_logprobs
# must be at most 11`, `max_tokens must be at least 0`, ...) and its do_POST catches nothing, so
# http.server's handle_error printed a traceback and closed the socket: the client read a transport
# failure, not an answer (Q-177: `top_logprobs` 12 on 8091). Other fields mlx_lm never reads (`n`,
# `response_format`) were answered as if honoured, and some it reads crash every rank AFTER the
# request is shared: a stop word that is not a string raises in `_make_state_machine`, which the
# batch path calls outside its try — the generation thread of every rank dies; a `seed` sends the
# request to `_serve_single`, which raises NotImplementedError on rank 0 alone when the answer is
# stopped early on a distributed group, so the ranks part mid-collective. Each is refused here with
# the field it names (`param`) and, where the engine has one, its limit.


class RequestRefused(ValueError):
    """A request field this engine cannot honour, named. `code`: unsupported_parameter (a value
    the OpenAI API accepts and this engine cannot serve) | invalid_value | missing_required_parameter."""

    def __init__(self, param, message, code):
        super().__init__(message)
        self.param = param
        self.code = code


PROMPT_FIELD = {
    "/v1/chat/completions": "messages",
    "/chat/completions": "messages",
    "/v1/completions": "prompt",
}


def refused_request_fields(path, body):
    """Raises RequestRefused for the first field of `body` (a POST to `path`) the distributed
    engine cannot honour; returns None when it can serve every field it was sent."""
    field = PROMPT_FIELD.get(path)
    if field is not None and field not in body:
        raise RequestRefused(field, f"{field} is required", "missing_required_parameter")
    if field == "messages" and not isinstance(body["messages"], list):
        raise RequestRefused(
            "messages",
            f"messages must be a list of message objects, not {type(body['messages']).__name__}",
            "invalid_value",
        )
    n = body.get("n")
    if n is not None:
        if not isinstance(n, int) or isinstance(n, bool) or n < 1:
            raise RequestRefused("n", f"n must be a positive integer, not {n!r}", "invalid_value")
        if n > 1:
            raise RequestRefused(
                "n",
                f"n={n}: the distributed engine writes one choice per request (n must be 1)",
                "unsupported_parameter",
            )
    response_format = body.get("response_format")
    if response_format is not None:
        if not isinstance(response_format, dict):
            raise RequestRefused(
                "response_format",
                f"response_format must be an object, not {type(response_format).__name__}",
                "invalid_value",
            )
        if response_format.get("type") != "text":
            raise RequestRefused(
                "response_format",
                f"response_format type {response_format.get('type')!r}: the distributed engine "
                "does not constrain its output; only {\"type\": \"text\"} is served",
                "unsupported_parameter",
            )
    stop = body.get("stop")
    if stop is not None and not isinstance(stop, str):
        if not isinstance(stop, list):
            raise RequestRefused(
                "stop",
                f"stop must be a string or a list of strings, not {type(stop).__name__}",
                "invalid_value",
            )
        for position, word in enumerate(stop):
            if not isinstance(word, str):
                raise RequestRefused(
                    "stop",
                    f"stop[{position}] must be a string, not {type(word).__name__}",
                    "invalid_value",
                )
            if not word:
                # An empty sequence matches at the trie's root: the answer would end after its
                # first token, as if the model had stopped.
                raise RequestRefused(
                    "stop", f"stop[{position}] is empty: a stop sequence needs text", "invalid_value"
                )
    if body.get("seed") is not None:
        raise RequestRefused(
            "seed",
            f"seed={body['seed']!r}: mlx_lm serves a seeded request outside the batch, and on a "
            "distributed group it cannot end that answer early without the ranks parting "
            "(NotImplementedError on rank 0 alone); send no seed",
            "unsupported_parameter",
        )
    stream_options = body.get("stream_options")
    if stream_options is not None:
        if not isinstance(stream_options, dict):
            raise RequestRefused(
                "stream_options",
                f"stream_options must be an object, not {type(stream_options).__name__}",
                "invalid_value",
            )
        include_usage = stream_options.get("include_usage")
        if include_usage is not None and not isinstance(include_usage, bool):
            raise RequestRefused(
                "stream_options",
                f"stream_options.include_usage must be a boolean, not {include_usage!r}",
                "invalid_value",
            )
    return None


# mlx_lm's validator names its own attribute; the client sent the body field.
VALIDATOR_FIELD = {"requested_model": "model", "adapter": "adapters"}


def validator_refusal(body, invalid):
    """mlx_lm's `<name> must be ...` ValueError as a RequestRefused naming the body field. The
    message is mlx_lm's own, so the limit it states is the engine's (top_logprobs: at most 11)."""
    message = str(invalid)
    name, sep, _ = message.partition(" must be ")
    if not sep or not name.isidentifier():
        return RequestRefused(None, message, "invalid_value")
    if name == "max_tokens" and body.get("max_completion_tokens") is not None:
        name = "max_completion_tokens"
    return RequestRefused(VALIDATOR_FIELD.get(name, name), message, "invalid_value")
