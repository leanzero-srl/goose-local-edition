"""Hidden contract tests. These never exist in the workspace during the run — they are dropped into
a snapshot afterwards, so the agent cannot read them, satisfy them by accident, or edit them."""

import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent


def run(*args, store="s.json"):
    return subprocess.run([sys.executable, "-m", "taskq", "--store", store, *args],
                          cwd=ROOT, capture_output=True, text=True, timeout=60)


def test_help_works():
    assert run("--help").returncode == 0 or subprocess.run(
        [sys.executable, "-m", "taskq", "--help"], cwd=ROOT,
        capture_output=True, text=True, timeout=60).returncode == 0


def test_add_then_list_json_has_the_exact_contract_keys(tmp_path):
    store = str(tmp_path / "a.json")
    assert run("add", "write spec", "--priority", "2", store=store).returncode == 0
    out = run("list", "--format", "json", store=store)
    assert out.returncode == 0
    rows = json.loads(out.stdout)
    assert isinstance(rows, list) and rows
    assert set(rows[0]) == {"id", "title", "priority", "done"}
    assert rows[0]["title"] == "write spec"
    assert rows[0]["priority"] == 2
    assert rows[0]["done"] is False


def test_list_is_ordered_by_priority_then_id(tmp_path):
    store = str(tmp_path / "b.json")
    run("add", "low", "--priority", "9", store=store)
    run("add", "high", "--priority", "1", store=store)
    run("add", "mid", "--priority", "5", store=store)
    rows = json.loads(run("list", "--format", "json", store=store).stdout)
    assert [r["title"] for r in rows] == ["high", "mid", "low"]


def test_done_marks_complete_and_purge_removes_it(tmp_path):
    store = str(tmp_path / "c.json")
    run("add", "one", store=store)
    rows = json.loads(run("list", "--format", "json", store=store).stdout)
    assert run("done", str(rows[0]["id"]), store=store).returncode == 0
    rows = json.loads(run("list", "--format", "json", store=store).stdout)
    assert rows[0]["done"] is True
    assert run("purge", store=store).returncode == 0
    assert json.loads(run("list", "--format", "json", store=store).stdout) == []


def test_default_priority_is_three(tmp_path):
    store = str(tmp_path / "d.json")
    run("add", "plain", store=store)
    rows = json.loads(run("list", "--format", "json", store=store).stdout)
    assert rows[0]["priority"] == 3


@pytest.mark.parametrize("args", [
    ("done", "99999"),
    ("add", "x", "--priority", "notanumber"),
    ("list", "--format", "yaml"),
])
def test_invalid_input_exits_nonzero(tmp_path, args):
    assert run(*args, store=str(tmp_path / "e.json")).returncode != 0
