"""The data contracts shared across brain / runner / collector / verifier (pydantic v2)."""

from __future__ import annotations

from typing import Any, Dict, List, Literal, Optional

from pydantic import BaseModel, Field


class Requirement(BaseModel):
    id: str
    text: str
    weight: int = 1
    kind: Literal["functional", "quality", "constraint", "regression"] = "functional"
    # A hint at how to verify deterministically (free text; the judge also reads it).
    check_hint: Optional[str] = None


class DeterministicCheck(BaseModel):
    type: Literal[
        "file_exists", "file_contains", "file_matches", "command_succeeds", "tool_called"
    ]
    name: Optional[str] = None
    path: Optional[str] = None
    regex: Optional[str] = None
    command: Optional[str] = None
    tool: Optional[str] = None


class TaskSpec(BaseModel):
    """One turn's instruction to the swarm, plus how to verify it."""

    task_id: str
    archetype: str
    app_slug: str
    workspace: str
    prompt: str
    language: str = "python"
    expected_mcp: List[str] = Field(default_factory=list)
    requirements: List[Requirement] = Field(default_factory=list)
    # Requirements the swarm never sees (minimal-spec gap-filling); scored by the judge.
    hidden_requirements: List[Requirement] = Field(default_factory=list)
    deterministic_checks: List[DeterministicCheck] = Field(default_factory=list)
    turn: int = 1
    persona: Optional[str] = None
    seed: int = 0


class ToolCall(BaseModel):
    name: str
    is_mcp: bool = False
    ok: Optional[bool] = None


class TaskOutcome(BaseModel):
    task_id: str
    status: str = "incomplete"
    device: Optional[str] = None
    model: Optional[str] = None
    attempts: int = 0
    elapsed_ms: Optional[int] = None
    session_id: Optional[str] = None
    tool_calls: List[ToolCall] = Field(default_factory=list)
    output: Optional[str] = None


class RunResult(BaseModel):
    """Parsed from `goose swarm run --output-format json` (the enriched RunReport)."""

    done: List[str] = Field(default_factory=list)
    failed: List[str] = Field(default_factory=list)
    results: Dict[str, str] = Field(default_factory=dict)
    dispatched_per_device: Dict[str, int] = Field(default_factory=dict)
    tasks: List[TaskOutcome] = Field(default_factory=list)
    per_device: Dict[str, Dict[str, Any]] = Field(default_factory=dict)
    exit_code: int = 0
    jsonl_path: Optional[str] = None
    stderr_tail: Optional[str] = None


class FileEntry(BaseModel):
    path: str
    bytes: int
    sha256: str


class Evidence(BaseModel):
    """Everything the verifier needs: the run result, the event log, traces, and built files."""

    task: TaskSpec
    run: RunResult
    events: List[Dict[str, Any]] = Field(default_factory=list)
    session_traces: List[Dict[str, Any]] = Field(default_factory=list)
    files: List[FileEntry] = Field(default_factory=list)


class DimensionVerdict(BaseModel):
    status: Literal["pass", "partial", "fail", "warn", "skip"]
    source: Literal["deterministic", "cluster", "judge"]
    detail: Optional[Any] = None


class Finding(BaseModel):
    id: str
    dimension: str
    severity: Literal["low", "medium", "high"]
    text: str
    evidence: Optional[str] = None
    fix_hint: Optional[str] = None


class Verdict(BaseModel):
    turn: int = 1
    overall: Literal["pass", "partial", "fail"] = "partial"
    dimensions: Dict[str, DimensionVerdict] = Field(default_factory=dict)
    findings: List[Finding] = Field(default_factory=list)
    judge_model: Optional[str] = None


class KnobChange(BaseModel):
    knob: str
    from_value: Any = None
    to_value: Any = None


class KnobDelta(BaseModel):
    rationale: str = ""
    changes: List[KnobChange] = Field(default_factory=list)
    # False ⇒ the change touches non-swarm core ⇒ the tweaker must reject it.
    scope_ok: bool = True
