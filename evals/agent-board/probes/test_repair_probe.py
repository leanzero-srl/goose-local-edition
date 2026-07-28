"""Adversarial tests for the repair probe — the ways a run could cheat it or break it.

The controls in meta.yaml prove the probe grades an honest attempt correctly. These prove it
survives a dishonest or malformed one, which is the half that actually decides whether a published
number means anything.
"""

from __future__ import annotations

import shutil
from pathlib import Path

import pytest

import repair

FIXTURE = Path(__file__).resolve().parents[1] / "verticals/repair/fixtures/slugkit-easy"
REFERENCE = FIXTURE / "controls/reference"


@pytest.fixture(scope="module")
def baseline():
    return repair.seed_baseline(FIXTURE, repair._load_meta(FIXTURE))


def workspace(tmp_path: Path, *, fixed: bool = False) -> Path:
    ws = tmp_path / "ws"
    shutil.copytree(FIXTURE / "seed", ws)
    if fixed:
        shutil.copytree(REFERENCE, ws, dirs_exist_ok=True)
    return ws


def test_rewriting_the_test_to_pass_is_caught(tmp_path, baseline):
    """The obvious cheat: make the failing assertion vacuous."""
    ws = workspace(tmp_path)
    (ws / "tests/test_slugkit.py").write_text(
        "from slugkit import slugify\n\n\ndef test_max_len_never_ends_with_hyphen():\n    assert True\n"
    )
    got = repair.grade(FIXTURE, ws, baseline)
    assert got["tampered"] is True
    assert got["score"] == 0.0
    assert "tests/test_slugkit.py" in got["tampered_files"]


def test_deleting_the_failing_test_is_caught(tmp_path, baseline):
    """The subtler cheat: a suite with no failing test is a green suite."""
    ws = workspace(tmp_path)
    (ws / "tests/test_slugkit.py").unlink()
    got = repair.grade(FIXTURE, ws, baseline)
    assert got["tampered"] is True
    assert got["score"] == 0.0


def test_a_real_fix_survives_the_restore(tmp_path, baseline):
    """Restoring pristine tests must not disturb an honest fix."""
    got = repair.grade(FIXTURE, workspace(tmp_path, fixed=True), baseline)
    assert got["score"] == 1.0
    assert got["tampered"] is False
    assert got["regressions"] == []


def test_tests_the_agent_added_do_not_score(tmp_path, baseline):
    """A test the run invented is reported, never rewarded and never punished."""
    ws = workspace(tmp_path, fixed=True)
    (ws / "tests/test_extra.py").write_text("def test_added_and_failing():\n    assert False\n")
    got = repair.grade(FIXTURE, ws, baseline)
    assert got["score"] == 1.0
    assert got["regressions"] == []
    assert any("test_added_and_failing" in t for t in got["added_tests"])


def test_nested_tree_is_still_found(tmp_path, baseline):
    """Agents routinely nest the project one level down; that is untidy, not a failure."""
    outer = tmp_path / "outer"
    outer.mkdir()
    shutil.copytree(FIXTURE / "seed", outer / "project")
    shutil.copytree(REFERENCE, outer / "project", dirs_exist_ok=True)
    got = repair.grade(FIXTURE, outer, baseline)
    assert got["score"] == 1.0


def test_deleted_source_scores_zero_without_crashing(tmp_path, baseline):
    ws = workspace(tmp_path)
    shutil.rmtree(ws / "slugkit")
    got = repair.grade(FIXTURE, ws, baseline)
    assert got["score"] == 0.0
    assert got["target_passed"] is False


def test_empty_workspace_scores_zero(tmp_path, baseline):
    ws = tmp_path / "empty"
    ws.mkdir()
    got = repair.grade(FIXTURE, ws, baseline)
    assert got["score"] == 0.0
    assert "layout-broken" in got["reason"]


def test_build_artefacts_do_not_trip_the_tamper_check(tmp_path, baseline):
    """The trap that cost a whole re-score last time: leftover run state read as a failure."""
    ws = workspace(tmp_path, fixed=True)
    (ws / ".pytest_cache").mkdir()
    (ws / ".pytest_cache/CACHEDIR.TAG").write_text("x")
    (ws / "slugkit/__pycache__").mkdir()
    (ws / "slugkit/__pycache__/core.pyc").write_bytes(b"\x00")
    (ws / "notes.md").write_text("scratch")
    got = repair.grade(FIXTURE, ws, baseline)
    assert got["tampered"] is False
    assert got["score"] == 1.0


def test_grading_is_deterministic(tmp_path, baseline):
    ws = workspace(tmp_path, fixed=True)
    assert repair.grade(FIXTURE, ws, baseline) == repair.grade(FIXTURE, ws, baseline)


def test_seed_has_exactly_one_failing_test(baseline):
    failing = [t for t, o in baseline.items() if o != "passed"]
    assert len(failing) == 1, failing
