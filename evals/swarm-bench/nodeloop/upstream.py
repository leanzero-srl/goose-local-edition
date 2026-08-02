#!/usr/bin/env python3
"""What has UPSTREAM (block/goose) shipped that we have not looked at yet? Exit 0.

Mihai, standing order 3c (2026-08-02): *"continuous investigation and research from other agents how
they're doing this better, like opencode, this fork's upper parent and see if they implemented
something new since we forked."*

A one-shot research sweep answers that once. This makes it a RATCHET: it records which upstream
commits have already been triaged in `upstream-seen.json`, so every later tick reads only what is
NEW. Without that, each tick either re-reads 252 commits (and so, in practice, reads none of them) or
skips the check entirely — which is exactly how "continuous research" decays into an intention.

The same treatment the boundary got: not a note to myself, a thing that counts.

RELEVANCE IS A FILTER, NOT A JUDGEMENT. This does not decide whether a commit is a good idea — it
decides whether it is even in scope, by path and by subject keyword. The swarm runs N IDENTICAL local
27B models on one box, so a new cloud provider, a UI change or a doc edit cannot help us however
good it is. What CAN help is anything touching the agent loop on a weak model: context/compaction,
tool schemas (every token of schema is a token not spent on the task), truncation, retry and timeout
policy, prompt construction, subagents and task delegation, streaming.

Usage:
    python3 upstream.py              # show NEW relevant commits since the last triage
    python3 upstream.py --all        # ignore the seen-list; show every relevant commit
    python3 upstream.py --mark <sha> [<sha>...]   # record shas as triaged
    python3 upstream.py --mark-all   # record every currently-listed commit as triaged
"""
from __future__ import annotations

import json
import pathlib
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[2]
SEEN = HERE / "upstream-seen.json"

# Paths where a change could plausibly affect how a WEAK LOCAL model behaves inside the agent loop.
# Deliberately excludes ui/, documentation/, and provider files for cloud vendors we do not run.
RELEVANT_PATHS = [
    "crates/goose/src/agents",
    "crates/goose/src/context_mgmt",
    "crates/goose/src/prompts",
    "crates/goose/src/hints",
    "crates/goose/src/session",
    "crates/goose/src/tool_",
    "crates/goose-mcp",
]

# Subject-line keywords that matter regardless of path. Each is here because it maps to a measured
# defect in this loop: token budget (a 27B's compliance collapses as rules grow), tool-result
# truncation (a measured infinite re-cat loop), retry/timeout (the 420s spin floor), subagents
# (the whole swarm), streaming (the event-silent tail).
RELEVANT_WORDS = [
    "subagent", "sub-agent", "agent loop", "compact", "context", "truncat", "token",
    "tool schema", "schema", "retry", "timeout", "prompt", "delegat", "parallel",
    "stream", "recipe", "planner", "structured output", "max turns", "spiral",
]

# Provably out of scope for a local-model agent loop, however much they touch a relevant path.
# `otel`/telemetry changes the observability of a run, not its behaviour; `ci:` never ships.
IGNORE_WORDS = ["docs:", "chore(deps", "bump ", "typo", "changelog", "release ", "ui:", "desktop:",
                "otel", "telemetry", "ci:", "ci(", "test:", "tests:"]


def sh(*args: str) -> str:
    return subprocess.run(args, capture_output=True, text=True, cwd=str(ROOT)).stdout


def load_seen() -> set[str]:
    if SEEN.is_file():
        try:
            return set(json.loads(SEEN.read_text()).get("triaged", []))
        except Exception:
            return set()
    return set()


def relevant(sha: str, subject: str) -> str | None:
    low = subject.lower()
    if any(w in low for w in IGNORE_WORDS):
        return None
    files = sh("git", "show", "--name-only", "--format=", sha)
    for p in RELEVANT_PATHS:
        if p in files:
            return f"path {p}"
    for w in RELEVANT_WORDS:
        if w in low:
            return f"subject '{w}'"
    return None


def main(argv: list[str]) -> int:
    seen = load_seen()
    if argv and argv[0] == "--mark":
        seen |= set(argv[1:])
        SEEN.write_text(json.dumps({"triaged": sorted(seen)}, indent=1))
        print(f"triaged {len(argv) - 1} commit(s); {len(seen)} total on the seen-list")
        return 0

    sh("git", "fetch", "upstream")
    mb = sh("git", "merge-base", "HEAD", "upstream/main").strip()
    if not mb:
        print("no upstream remote or no merge-base — cannot diff against the parent project")
        return 0
    raw = sh("git", "log", "--oneline", "--no-merges", f"{mb}..upstream/main")
    commits = []
    for line in raw.splitlines():
        if not line.strip():
            continue
        sha, _, subject = line.partition(" ")
        commits.append((sha, subject))

    show_all = "--all" in argv
    hits = []
    for sha, subject in commits:
        if not show_all and sha in seen:
            continue
        why = relevant(sha, subject)
        if why:
            hits.append((sha, subject, why))

    print(f"=== UPSTREAM WATCH — {len(commits)} commit(s) since fork-point {mb[:12]} ===")
    print(f"{len(seen)} already triaged; {len(hits)} relevant and NOT yet triaged\n")
    if not hits:
        print("nothing new in scope. That is a real answer — record it and move on; do NOT go")
        print("hunting for something to adopt just because the check ran.")
        return 0
    for sha, subject, why in hits:
        print(f"  {sha}  {subject[:96]}")
        print(f"           ^ in scope by {why}")
    print()
    print("Read the diffs before believing any of these apply: `git show <sha>`. Then either adopt or")
    print("record as triaged so it never costs a second look:")
    print(f"  python3 upstream.py --mark {' '.join(s for s, _, _ in hits[:6])}")
    if "--mark-all" in argv:
        seen |= {s for s, _, _ in hits}
        SEEN.write_text(json.dumps({"triaged": sorted(seen)}, indent=1))
        print(f"\n--mark-all: recorded {len(hits)} commit(s) as triaged")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
