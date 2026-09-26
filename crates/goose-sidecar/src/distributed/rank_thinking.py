# goose distributed rank: the thinking a chat request gets, resolved the way goose's single engine
# resolves it. Pure stdlib; concatenated after rank_live.py, before the rank program (both kinds).
#
# goose sends `chat_template_kwargs.enable_thinking` only when the model's profile pins thinking on
# or off; "auto" sends nothing and the ENGINE decides (engine.rs `ModelProfile::thinking`). Rapid-MLX,
# the single engine, decides OFF for every request goose makes: /v1/chat/completions turns thinking
# off on a request that carries tools (service/helpers.py `maybe_auto_disable_thinking_for_tools`)
# and on a tool-less one to a thinking-capable model (`..._for_casual_chat`), unless the request pins
# `enable_thinking` or signals reasoning intent. mlx_lm.server and the fork's pipeline server have
# no such rule: the absent switch reached Qwen3.8's template as undefined, which it renders as ON at
# effort xhigh — a 209-char "Reasoning effort is set to xhigh…" system line and a bare `<think>\n`
# generation prompt where the single engine renders `<think>\n\n</think>\n\n` (Q-135, 2026-09-26:
# the split's first agent call of the Jira brief thought 7k and 10.4k+ tokens, the single engine's
# 65–817 tokens with none). So every rank resolves the switch here and hands the template an
# explicit boolean — the same render as the single engine for the same request.
#
# What the single engine translates and a split cannot (top-level graded `reasoning_effort`, a
# `reasoning_max_tokens` cap, the Responses `reasoning` dict) is refused by name, never dropped.

# rapid_mlx/distributed/pipeline_qwen4_serve.py `_TOOL_XML_MARKERS` / `_THINK_MARKERS`: a template
# carrying both contracts gets the engine's `deepseek_r1` reasoning parser (goose's model_parsers.rs
# wires the same parser into the single engine's argv for the same template).
TOOL_XML_MARKERS = (
    "tool_calls",
    "arguments",
    "<tool_call>",
    "</tool_call>",
    "<function=",
    "</function>",
    "<parameter=",
    "</parameter>",
)
THINK_MARKERS = ("enable_thinking", "<think>", "</think>")


class ThinkingRefused(ValueError):
    """The request asks for a reasoning control this engine cannot honour."""


def template_reasons(template):
    """Whether the engine serving `template` runs a reasoning parser — the single engine's
    precondition for turning thinking off on a tool-less request."""
    if not isinstance(template, str):
        return False
    return all(m in template for m in TOOL_XML_MARKERS) and all(
        m in template for m in THINK_MARKERS
    )


def pinned_thinking(body):
    """`_extract_thinking_from_request`: chat_template_kwargs.enable_thinking (a bool, or the
    strings "true"/"false"), else the top-level `enable_thinking`, else None."""
    kwargs = body.get("chat_template_kwargs")
    if isinstance(kwargs, dict) and "enable_thinking" in kwargs:
        value = kwargs["enable_thinking"]
        if isinstance(value, bool):
            return value
        if isinstance(value, str) and value.strip().lower() in ("true", "false"):
            return value.strip().lower() == "true"
    top = body.get("enable_thinking")
    return top if isinstance(top, bool) else None


def refused_reasoning_controls(body):
    """The reasoning controls the single engine translates and this one would silently drop."""
    refused = []
    effort = body.get("reasoning_effort")
    if effort is not None and effort != "none":
        refused.append(f"reasoning_effort={effort!r}")
    if body.get("reasoning_max_tokens") is not None:
        refused.append(f"reasoning_max_tokens={body['reasoning_max_tokens']!r}")
    reasoning = body.get("reasoning")
    if isinstance(reasoning, dict) and reasoning.get("effort") is not None:
        refused.append(f"reasoning={reasoning!r}")
    return refused


def resolved_thinking(body, reasons):
    """The single engine's answer for one chat request (routes/chat.py: effort "none", the tools
    gate, the casual gate, then `_resolve_enable_thinking`): True/False, or None = the template's
    own default. `reasons` = `template_reasons` of the served template."""
    refused = refused_reasoning_controls(body)
    if refused:
        raise ThinkingRefused(
            f"{', '.join(refused)}: the distributed engine does not translate these reasoning "
            "controls; pin chat_template_kwargs.enable_thinking (and chat_template_kwargs."
            "reasoning_effort, a level of the model's own template) instead"
        )
    pinned = pinned_thinking(body)
    if pinned is not None:
        return pinned
    if body.get("reasoning_effort") == "none":
        return False
    if body.get("tools"):
        return None if body.get("tool_choice") == "none" else False
    return False if reasons else None


def resolved_template_kwargs(body, reasons):
    """The chat_template_kwargs the template renders this request with: the client's own, with
    `enable_thinking` as the resolved boolean (absent only when the template's default stands)."""
    kwargs = body.get("chat_template_kwargs")
    kwargs = dict(kwargs) if isinstance(kwargs, dict) else {}
    thinking = resolved_thinking(body, reasons)
    if thinking is None:
        kwargs.pop("enable_thinking", None)
    else:
        kwargs["enable_thinking"] = thinking
    return kwargs
