# goose distributed tensor rank: the sampling a request runs with when it names none, resolved the
# way goose's single engine resolves it (Q-159). Pure stdlib; concatenated after rank_thinking.py,
# before the tensor program (the pipeline fork resolves its own since lz-pipeline-qwen4.9).
#
# mlx_lm.server 0.31.3 fills an absent temperature with its `--temp` default, 0.0
# (server.py:1817-1822; top_p 1.0, top_k 0, min_p 0.0, penalties 0.0 at server.py:1174-1185), and
# goose names no sampling field, so every split request decoded GREEDY. E2E #3d (2026-09-26): one
# answer was 57 tool calls, 54 of them `ledger__ledger_append` with the identical argument, over 40
# minutes; Qwen's card warns greedy decoding repeats endlessly. Rapid-MLX (the single engine,
# v0.14.3-lz.9) resolves each field request > `--default-*` (goose's per-model profile, engine.rs)
# > the alias catalog > generation_config.json > a fallback (service/helpers.py `_resolve_*`,
# `_cascade`; utils/generation_config.py): Qwen3.8's config gives temperature 1.0, top_k 20,
# top_p 0.95. The alias layer never applies to goose — goose serves a local directory, and
# `resolve_profile` matches only alias names and HF ids — so the chain here is request > profile
# (the spec's `sampling_defaults`) > generation_config.json > the single engine's fallback.
#
# Where the single engine skips a missing or unreadable generation_config.json silently, rank 0
# names it: GOOSE_RANK_GENERATION_CONFIG_UNREAD in its log and `sampling_defaults.
# generation_config_error` on /v1/status — and the fallback in force is the single engine's, never
# greedy.
import json
import math
import os

SAMPLING_KEYS = (
    "temperature",
    "top_p",
    "top_k",
    "min_p",
    "repetition_penalty",
    "presence_penalty",
    "frequency_penalty",
)
# rapid_mlx/service/helpers.py `_FALLBACK_TEMPERATURE` / `_FALLBACK_TOP_P` (v0.14.3-lz.9): the single
# engine's last layer; top_k, min_p and the penalties have none there (unset = the sampler's off).
SINGLE_ENGINE_FALLBACK = {"temperature": 0.7, "top_p": 0.9}


def generation_config_sampling(model_dir):
    """(path, values, ignored, error) for `<model_dir>/generation_config.json`: its sampling
    subset, kept as utils/generation_config.py keeps it (numbers only — no bool, NaN or infinity —
    and a whole-number top_k as an int); `ignored` names each sampling key that filter drops;
    `error` names why no values could be read (absent, unreadable, not a JSON object)."""
    path = os.path.join(os.path.expanduser(model_dir), "generation_config.json")
    try:
        with open(path) as handle:
            raw = json.load(handle)
    except FileNotFoundError:
        return path, {}, [], f"{path} is absent"
    except (OSError, ValueError) as unread:
        return path, {}, [], f"{path} is unreadable: {unread}"
    if not isinstance(raw, dict):
        return path, {}, [], f"{path} holds no JSON object"
    values, ignored = {}, []
    for key in SAMPLING_KEYS:
        if key not in raw:
            continue
        value = raw[key]
        if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
            ignored.append(f"{key}={value!r}")
            continue
        if key == "top_k":
            if isinstance(value, float) and not value.is_integer():
                ignored.append(f"{key}={value!r}")
                continue
            value = int(value)
        values[key] = value
    return path, values, ignored, None


class SamplingDefaults:
    """Rank 0's layers under a request's own sampling fields."""

    def __init__(self, profile, model_dir):
        self.profile = {
            key: value for key, value in (profile or {}).items() if key in SAMPLING_KEYS and value is not None
        }
        self.path, self.generation_config, self.ignored, self.error = generation_config_sampling(model_dir)

    def report(self):
        """`/v1/status`'s `sampling_defaults`, and GOOSE_RANK_SAMPLING_DEFAULTS at startup."""
        return {
            "profile": dict(self.profile),
            "generation_config": dict(self.generation_config),
            "generation_config_path": self.path,
            "generation_config_error": self.error,
            "generation_config_ignored": list(self.ignored),
            "engine_fallback": dict(SINGLE_ENGINE_FALLBACK),
        }

    def resolve(self, body):
        """{key: (value, layer)} for every sampling field of one request. A request's null is no
        value (the single engine's pydantic field is None either way). layer: request | profile |
        generation_config | engine_fallback | unset (value None: the sampler's own off)."""
        resolved = {}
        for key in SAMPLING_KEYS:
            if body.get(key) is not None:
                resolved[key] = (body[key], "request")
            elif key in self.profile:
                resolved[key] = (self.profile[key], "profile")
            elif key in self.generation_config:
                resolved[key] = (self.generation_config[key], "generation_config")
            elif key in SINGLE_ENGINE_FALLBACK:
                resolved[key] = (SINGLE_ENGINE_FALLBACK[key], "engine_fallback")
            else:
                resolved[key] = (None, "unset")
        return resolved


def sampling_row(args, sources):
    """A request's `/v1/status` `sampling`: what reached mlx_lm's sampler and logits processors
    (the GenerationArguments rank 0 shares with every rank), and the layer each came from."""
    reached = {
        "temperature": args.sampling.temperature,
        "top_p": args.sampling.top_p,
        "top_k": args.sampling.top_k,
        "min_p": args.sampling.min_p,
        "repetition_penalty": args.logits.repetition_penalty,
        "presence_penalty": args.logits.presence_penalty,
        "frequency_penalty": args.logits.frequency_penalty,
    }
    return {
        key: {"value": value, "from": (sources or {}).get(key)} for key, value in reached.items()
    }
