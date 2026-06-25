"""The generator: invents opener tasks (3 archetypes) and vibes follow-up turns like a real user."""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Dict, List, Optional

from .brain.base import Brain
from .contracts import DeterministicCheck, Requirement, TaskSpec

_OUT_SHAPE = (
    '{"app_slug":"kebab-name","language":"python|rust|node","prompt":"<the exact message to send '
    'the swarm>","requirements":[{"id":"R1","text":"...","kind":"functional|quality|constraint",'
    '"weight":1,"check_hint":"..."}],"hidden_requirements":[...same shape...],'
    '"deterministic_checks":[{"type":"file_exists|file_contains|file_matches|command_succeeds|'
    'tool_called","name":"builds","path":"...","regex":"...","command":"...","tool":"..."}],'
    '"expected_mcp":[]}'
)

ARCHETYPES: Dict[str, str] = {
    "heavy-spec": (
        "Invent a NEW small, self-contained app idea, then write a DETAILED spec for it. The `prompt` "
        "is what we send the coding swarm: state the app, explicit features, the file/CLI surface, and "
        "acceptance criteria. Populate MANY concrete `requirements` and matching `deterministic_checks` "
        "(include a build/run check and a test command). Keep it buildable in one shot."
    ),
    "minimal-spec": (
        "Invent a NEW small app whose `prompt` is a TERSE one-liner a lazy user would type (few words, "
        "lots unsaid) AND that genuinely benefits from looking things up — library docs or web facts — so "
        "the swarm must use its MCP tools. Set `expected_mcp` to the servers it should use (context7 for "
        "library docs, web-search for facts). Put what a reasonable engineer would infer into "
        "`hidden_requirements` (the swarm never sees these): persistence, a CLI/entry point, input "
        "validation, a README, tests, and CORRECT use of anything looked up. Keep `requirements` minimal; "
        "add a few light `deterministic_checks` (build/run + a tool_called check for the MCP tool)."
    ),
    "continue-existing": (
        "You are continuing work on an EXISTING app (its files are given). Invent a feature request that "
        "REQUIRES understanding the existing code; `prompt` must NOT restate the codebase — make the swarm "
        "investigate it. Add `requirements` for the new feature AND `regression` requirements that the "
        "existing behavior/tests still pass. Add `deterministic_checks` incl. re-running existing tests."
    ),
}


def slugify(s: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")[:40] or "app"


def open_session(
    brain: Brain,
    archetype: str,
    persona: str,
    seed: int,
    apps_dir: Path,
    existing: Optional[dict] = None,
) -> TaskSpec:
    sys = (
        "You are a product-minded user driving an automated test of a local-AI coding agent. "
        + ARCHETYPES[archetype]
        + f"\nAdopt the persona: {persona}. Vary your idea by random seed {seed}. "
        + "Return JSON shaped like: "
        + _OUT_SHAPE
    )
    user = "Generate the opener."
    if archetype == "continue-existing" and existing:
        user = "Existing app to extend:\n" + json.dumps(existing, indent=2)[:8000]
    data = brain.json_call(sys, user, max_tokens=6000)

    slug = slugify(data.get("app_slug") or archetype)
    if existing and existing.get("app_slug"):
        slug = existing["app_slug"]
    workspace = apps_dir / slug
    return _to_taskspec(data, archetype, slug, workspace, persona, seed, turn=1)


def next_move(
    brain: Brain,
    task: TaskSpec,
    verdict_dump: dict,
    file_tree: List[str],
    persona: str,
    turn: int,
) -> dict:
    sys = (
        f"You are a {persona} user continuing a session with a local-AI coding agent on an existing app. "
        "Decide the NEXT realistic action and send it as `prompt`. Pick `kind` from: feature, fix "
        "(react to findings), tests, refactor, mcp-feature (needs web/docs lookup), change-direction. "
        "If the app is in good shape and you're satisfied, set done=true. Every move is an amendment on "
        "the EXISTING code; add `requirements` (incl. regression: prior behavior still works) and "
        "`deterministic_checks` for THIS move. Return JSON shaped like: "
        '{"prompt":"...","kind":"...","done":false,"requirements":[...],"deterministic_checks":[...],'
        '"hidden_requirements":[]}'
    )
    user = json.dumps(
        {
            "app_slug": task.app_slug,
            "language": task.language,
            "current_files": file_tree,
            "last_verdict": verdict_dump,
            "turn": turn,
        },
        indent=2,
    )[:30000]
    return brain.json_call(sys, user, max_tokens=4000)


def move_to_taskspec(move: dict, base: TaskSpec, turn: int) -> TaskSpec:
    return _to_taskspec(
        move, base.archetype, base.app_slug, Path(base.workspace), base.persona or "", base.seed, turn
    )


_REQ_KINDS = {"functional", "quality", "constraint", "regression"}
_CHK_TYPES = {"file_exists", "file_contains", "file_matches", "command_succeeds", "tool_called"}
_CHK_KEYS = {"type", "name", "path", "regex", "command", "tool"}


def _to_taskspec(data, archetype, slug, workspace, persona, seed, turn) -> TaskSpec:
    if not isinstance(data, dict):
        data = {}
    mcp = data.get("expected_mcp") or []
    return TaskSpec(
        task_id=f"{slug}-t{turn}",
        archetype=archetype,
        app_slug=slug,
        workspace=str(workspace),
        prompt=str(data.get("prompt", "")),
        language=str(data.get("language", "python")),
        expected_mcp=[str(m) for m in mcp if isinstance(m, (str, int))],
        requirements=_reqs(data.get("requirements")),
        hidden_requirements=_reqs(data.get("hidden_requirements")),
        deterministic_checks=_checks(data.get("deterministic_checks")),
        turn=turn,
        persona=persona,
        seed=seed,
    )


def _reqs(items) -> List[Requirement]:
    """Tolerate imperfect brain JSON: items may be dicts, plain strings, or junk."""
    out: List[Requirement] = []
    for r in items or []:
        if isinstance(r, str):
            r = {"text": r}
        if not isinstance(r, dict):
            continue
        kind = r.get("kind", "functional")
        if kind not in _REQ_KINDS:
            kind = "functional"
        try:
            out.append(
                Requirement(
                    id=str(r.get("id", f"R{len(out) + 1}")),
                    text=str(r.get("text", "")),
                    kind=kind,
                    weight=int(r.get("weight", 1) or 1),
                    check_hint=r.get("check_hint"),
                )
            )
        except Exception:
            pass
    return out


def _checks(items) -> List[DeterministicCheck]:
    out: List[DeterministicCheck] = []
    for c in items or []:
        if not isinstance(c, dict) or c.get("type") not in _CHK_TYPES:
            continue
        try:
            out.append(DeterministicCheck(**{k: v for k, v in c.items() if k in _CHK_KEYS}))
        except Exception:
            pass
    return out
