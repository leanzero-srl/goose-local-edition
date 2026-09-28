# goose distributed tensor rank: the type a tool parameter's schema names, read through a union or a
# reference, for mlx_lm's qwen3_coder parser (Q-232). Pure stdlib; concatenated after
# rank_boundary.py, before rank_tool_stream.py; rank_wrapper.py installs it over the parser's
# `_get_arguments_config`, so the whole-call parse and the streamer (rank_tool_stream.py) read it.
#
# mlx_lm 0.31.3's `_convert_param_value` reads a property's type as `str(param["type"])`: a string
# type keeps the value's raw text, int/num/bool/object/array convert, ANY OTHER text goes to
# `ast.literal_eval` — and a property with no "type" key at all is read as a string. So:
#   {"type": ["string", "null"]} + `10m`   -> literal_eval("10m"): SyntaxError, the call is lost
#     (streamed: its arguments never close; not streamed: the SyntaxError escapes mlx_lm's
#     ToolCallFormatter, which skips only ValueError, and the connection closes with no reply);
#   {"type": ["string", "null"]} + `42`    -> the integer 42; `'x'` -> the string x (quotes eaten);
#   {"type": ["boolean", "null"]} + `true` -> literal_eval("true"): ValueError, the call is dropped;
#   {"$ref": "#/$defs/CropParams"}         -> no type key: the object arrives as its JSON TEXT.
# goose's own requests reach the engine with top-level `[T, "null"]` already made `T`
# (goose-provider-types openai.rs `normalize_nullable`) — measured over 2,685 request logs, 86
# tools: no top-level type list on goose's wire — but `read_image.crop` is a `$ref` there, and any
# other client of this port sends what its own schema generator writes. Rapid-MLX's parser (the
# single engine and the pipeline fork, `api/tool_calling.py _schema_type`) already reads all of
# these as their type; this makes the tensor split read them the same way.
#
# The rule: a type list without "null"; anyOf/oneOf/allOf branches, each read by the same rule; a
# local `$ref` into the tool's `$defs`/`definitions`, followed. A union holding "string" is a
# string (its raw text is always a value it accepts — the only reading that never refuses); a union
# of exactly one other type is that type; anything else (two non-string types, an unresolvable
# reference, a branch that names no type) names NO type and the property reaches the parser as
# written — exactly what mlx_lm reads today. A property with a single "type" string, or none of
# these keys, is never touched.

import functools  # noqa: E402

_NULL = "null"
_STRING = "string"


def _union_type(types):
    """One type for a union's members (None where a member names none), or None."""
    named = {t for t in types if t != _NULL}
    if _STRING in named:
        return _STRING
    if None in named or len(named) != 1:
        return None
    return named.pop()


def schema_type(schema, defs, followed=()):
    """The single JSON type `schema` names (a declared "null" included), else None."""
    if not isinstance(schema, dict):
        return None
    declared = schema.get("type")
    if isinstance(declared, str):
        return declared
    if isinstance(declared, list):
        return _union_type([t if isinstance(t, str) else None for t in declared])
    ref = schema.get("$ref")
    if isinstance(ref, str):
        if ref in followed:
            return None
        return schema_type(_local_definition(ref, defs), defs, followed + (ref,))
    for key in ("anyOf", "oneOf", "allOf"):
        branches = schema.get(key)
        if isinstance(branches, list) and branches:
            return _union_type([schema_type(branch, defs, followed) for branch in branches])
    return None


def _local_definition(ref, defs):
    for prefix in ("#/$defs/", "#/definitions/"):
        if ref.startswith(prefix):
            return defs.get(ref[len(prefix):])
    return None


def _tool_definitions(func_name, tools):
    """The `$defs` and `definitions` of the named tool's parameters, as the parser finds the tool."""
    for tool in tools or ():
        function = tool.get("function") if isinstance(tool, dict) else None
        if not isinstance(function, dict) or function.get("name") != func_name:
            continue
        parameters = function.get("parameters")
        if not isinstance(parameters, dict):
            return {}
        defs = {}
        for key in ("definitions", "$defs"):
            if isinstance(parameters.get(key), dict):
                defs.update(parameters[key])
        return defs
    return {}


def typed_arguments_config(upstream):
    """`upstream` (the parser's `_get_arguments_config`) with each property whose type is read
    through a union or a reference given that type. The request's own tools are never changed."""

    @functools.wraps(upstream)
    def arguments_config(func_name, tools):
        properties = upstream(func_name, tools)
        if not isinstance(properties, dict):
            return properties
        defs = None
        typed = {}
        for name, prop in properties.items():
            if isinstance(prop, dict) and not isinstance(prop.get("type"), str):
                if defs is None:
                    defs = _tool_definitions(func_name, tools)
                resolved = schema_type(prop, defs)
                if resolved is not None and resolved != _NULL:
                    prop = {**prop, "type": resolved}
            typed[name] = prop
        return typed

    return arguments_config
