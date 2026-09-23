#!/usr/bin/env python3
"""Capture the goose-shaped agent fixture for workload (c) from ONE real goose request.

Usage: capture_goose_fixture.py <llm_request.N.jsonl> <out.json>

The source is a line-0 request body from ~/.local/state/goose/logs/llm_request.*.jsonl
(goose's own provider log): the real system prompt goose built and the real tool schemas it
sent. What is personal is REPLACED, never dropped, so the fixture keeps the measured
character length of every span (prompt size is what a cache/prefill bench measures):
  - the memory INDEX (the user's saved memory headlines),
  - the skills catalogue and the subagent catalogue (names of the user's own work),
  - the imported global-hints block (<!-- goose:import ... --> ... <!-- /goose:import -->),
  - any home-directory path.
Each replaced span is filled with neutral deterministic prose of the SAME character count,
and the report lists every span with its length. Nothing else is touched; the goose-authored
instructions and every tool schema stay byte-identical. The script refuses when a span it
expects is missing (a different goose build changed the prompt) instead of emitting a
half-sanitized fixture.
"""

import json
import re
import sys

FILLER_SENTENCES = [
    "The service reads its configuration once at startup and keeps it in memory.",
    "A request that fails validation is answered with a clear error and nothing is written.",
    "Every change is recorded in the project log together with the reason it was made.",
    "The build runs the unit tests first and the slower integration tests afterwards.",
    "When a dependency is unavailable the caller sees the failure instead of a default value.",
    "Documentation for each module lives next to the code it describes.",
    "The scheduler hands work to the next free worker and records when it finished.",
    "Reviewers read the diff, run the tests locally, and leave comments on specific lines.",
]


def filler(length: int, salt: int) -> str:
    out = []
    i = salt
    while sum(len(s) + 1 for s in out) < length:
        out.append(FILLER_SENTENCES[i % len(FILLER_SENTENCES)])
        i += 3
    text = " ".join(out)
    return text[:length]


def span_between(s: str, start_marker: str, end_marker: str, include_start: bool) -> tuple[int, int]:
    a = s.find(start_marker)
    if a < 0:
        sys.exit(f"refusing: marker not found: {start_marker!r}")
    if not include_start:
        a += len(start_marker)
    b = s.find(end_marker, a)
    if b < 0:
        sys.exit(f"refusing: end marker not found after {start_marker!r}: {end_marker!r}")
    return a, b


def main() -> None:
    src, out = sys.argv[1], sys.argv[2]
    with open(src) as f:
        body = json.loads(f.readline())["input"]
    system = body["messages"][0]
    assert system["role"] == "system", system["role"]
    s: str = system["content"]

    spans = [
        ("memory index", *span_between(s, "Memory index:\n", "\n## playwright", False)),
        ("skills catalogue", *span_between(s, "## skills\n\n### Instructions\n", "\n## summon", False)),
        ("subagent catalogue", *span_between(s, "## summon\n\n### Instructions\n", "\n## tom", False)),
        ("imported global hints", *span_between(s, "<!-- goose:import", "<!-- /goose:import -->", True)),
    ]
    spans.sort(key=lambda t: t[1])
    rebuilt, cursor, report = [], 0, []
    for salt, (name, a, b) in enumerate(spans):
        rebuilt.append(s[cursor:a])
        rebuilt.append(filler(b - a, salt))
        report.append({"span": name, "chars": b - a})
        cursor = b
    rebuilt.append(s[cursor:])
    sanitized = "".join(rebuilt)
    sanitized = re.sub(r"/Users/[A-Za-z0-9._-]+", "/Users/operator", sanitized)

    tools = json.loads(re.sub(r"/Users/[A-Za-z0-9._-]+", "/Users/operator", json.dumps(body["tools"])))
    fixture = {
        "source": "goose provider log line 0 (real system prompt + tool schemas), sanitized by capture_goose_fixture.py",
        "system_prompt_chars_original": len(s),
        "system_prompt_chars": len(sanitized),
        "replaced_spans": report,
        "tool_count": len(tools),
        "system_prompt": sanitized,
        "tools": tools,
    }
    with open(out, "w") as f:
        json.dump(fixture, f, indent=1, ensure_ascii=False)
    print(json.dumps({k: v for k, v in fixture.items() if k not in ("system_prompt", "tools")}, indent=1))


if __name__ == "__main__":
    main()
