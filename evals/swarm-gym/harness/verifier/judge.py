"""The AI judge: scores requirement coverage, code quality, and hunts subtle bugs."""

from __future__ import annotations

import json
from pathlib import Path

from ..brain.base import Brain
from ..contracts import Evidence

JUDGE_SYS = (
    "You are a meticulous senior engineer REVIEWING what a local-AI coding swarm produced for a user "
    "request. Judge three things and HUNT for real mistakes: (1) requirements — is each requirement "
    "actually met by the code (read it, don't assume); (2) code_quality — correctness, error handling, "
    "sensible structure (1-5); (3) bugs — off-by-ones, missing edge cases, race conditions, wrong "
    "logic, dead code. Be concrete and cite file:line in evidence. Do not reward effort; reward "
    "correctness. For a minimal-spec task, judge whether the swarm made reasonable engineering choices."
)

SCHEMA = (
    '{"requirements":{"status":"pass|partial|fail","covered":<int>,"total":<int>},'
    '"code_quality":{"status":"pass|partial|fail","score":<1-5>},'
    '"bugs":{"status":"pass|partial|fail"},'
    '"findings":[{"id":"F1","dimension":"requirements|code_quality|bugs",'
    '"severity":"low|medium|high","text":"...","evidence":"file:line","fix_hint":"..."}]}'
)


def judge(brain: Brain, ev: Evidence) -> dict:
    reqs = [r.model_dump() for r in (ev.task.requirements + ev.task.hidden_requirements)]
    payload = {
        "task_prompt": ev.task.prompt,
        "archetype": ev.task.archetype,
        "language": ev.task.language,
        "requirements": reqs,
        "swarm_done": ev.run.done,
        "swarm_failed": ev.run.failed,
        "files": _files_blob(ev),
    }
    user = json.dumps(payload, indent=2)[:60000]
    return brain.json_call(JUDGE_SYS + "\nReturn JSON shaped like: " + SCHEMA, user, max_tokens=6000)


def _files_blob(ev: Evidence, cap: int = 28000) -> str:
    parts, used = [], 0
    ws = Path(ev.task.workspace)
    for f in ev.files:
        try:
            txt = (ws / f.path).read_text(errors="replace")
        except Exception:
            continue
        chunk = f"\n### {f.path}\n{txt}"
        if used + len(chunk) > cap:
            chunk = chunk[: max(0, cap - used)]
        parts.append(chunk)
        used += len(chunk)
        if used >= cap:
            break
    return "".join(parts)
