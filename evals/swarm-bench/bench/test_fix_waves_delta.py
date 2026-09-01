"""The fix-wave value instrument: provenance is PROVEN from run.jsonl, the ledger line is exact, and a
cross-seed delta is refused. The r6c shape (round 1 overwrote round 0 in place) is pinned verbatim."""
import json
import pathlib
import sys

import pytest

sys.path.insert(0, str(pathlib.Path(__file__).parent))
import fix_waves_delta as fwd  # noqa: E402

SEED = "d32de873c1f2be83"
SNAP0 = {"event": "best_tree_snapshot", "round": 0, "findings": 9, "established": True, "ok": True,
         "ts": "2026-08-31T22:15:31.602697+00:00"}
SNAP1 = {"event": "best_tree_snapshot", "round": 1, "findings": 8, "established": True, "ok": True,
         "ts": "2026-08-31T23:42:21.309858+00:00"}
# r6c's real handover/dispatch stamps: phase repair 22:13:50Z, first complete_fix_dispatched 22:15:31.608581Z.
PFX_WRITE = {"event": "prefix_tree_snapshot", "ok": True, "files": 3, "ts": "2026-08-31T22:13:50.500000+00:00"}
FIX0 = {"event": "complete_fix_dispatched", "round": 0, "shard": "web/viz.js", "ts": "2026-08-31T22:15:31.608581+00:00"}


def _v(seed, inner, mult, score, crits):
    return {"fixture_seed": seed, "inner": inner, "score": score,
            "critical": {"multiplier": mult, "rows": [{"check": c} for c in crits]}}


def _run(tmp, events, best=True, prefix=False, best_files=None):
    run = tmp / "run"
    run.mkdir()
    (run / "run.jsonl").write_text("".join(json.dumps(e) + "\n" for e in events))
    (run / "app").mkdir()
    (run / "app/x.py").write_text("print(1)\n")
    if best:
        b = run / ".swarm/best-tree"
        b.mkdir(parents=True)
        for name, body in (best_files or {"app/x.py": "print(1)\n"}).items():
            (b / name).parent.mkdir(parents=True, exist_ok=True)
            (b / name).write_text(body)
    if prefix:
        p = run / ".swarm/prefix-tree/app"
        p.mkdir(parents=True)
        (p / "x.py").write_text("print(0)\n")
    return run


def test_line_is_the_exact_ledger_shape():
    p = _v(SEED, 0.7136, 0.216, 0.1392, ["b_money_rendered", "j_loads_data", "j_workflow_journey"])
    f = _v(SEED, 0.7282, 0.36, 0.2621, ["j_workflow_journey"])
    assert fwd.line(p, f) == (
        "fix_waves_delta: prefix 0.7136/0.216/0.1392 → final 0.7282/0.360/0.2621; "
        "criticals moved: closed [b_money_rendered, j_loads_data]")


def test_an_opened_critical_is_named_too():
    p = _v(SEED, 0.7, 0.36, 0.25, ["a"])
    f = _v(SEED, 0.7, 0.216, 0.15, ["a", "b"])
    assert fwd.line(p, f).endswith("criticals moved: opened [b]")


def test_no_movement_names_what_stayed():
    p = _v(SEED, 0.7, 0.216, 0.14, ["x", "y"])
    f = _v(SEED, 0.71, 0.216, 0.14, ["x", "y"])
    assert fwd.line(p, f).endswith("criticals moved: none (2 unsuppressed: x, y)")


def test_cross_seed_is_refused():
    with pytest.raises(SystemExit, match="different seeds"):
        fwd.line(_v("a" * 16, 0.7, 1, 0.7, []), _v("b" * 16, 0.7, 1, 0.7, []))


def test_round0_best_tree_is_the_prefix_tree(tmp_path):
    info = fwd.provenance(_run(tmp_path, [SNAP0]))
    assert info["label"] == "best-tree@r0"
    assert info["source"].endswith(".swarm/best-tree")


def test_the_r6c_shape_is_refused_with_the_overwrite_named(tmp_path):
    info = fwd.provenance(_run(tmp_path, [SNAP0, SNAP1]))
    assert info["source"] is None
    assert "round 1" in info["reason"] and "overwritten" in info["reason"]
    assert info["best_tree_identical_to_final"] is True
    assert "byte-identical" in info["reason"]


def test_a_surviving_snapshot_that_differs_says_by_how_much(tmp_path):
    info = fwd.provenance(_run(tmp_path, [SNAP0, SNAP1], best_files={"app/x.py": "print(2)\n"}))
    assert info["source"] is None
    assert info["best_tree_identical_to_final"] is False
    assert "differs from the final tree in 1 path(s)" in info["reason"]


def test_engine_prefix_tree_wins_over_best_tree(tmp_path):
    info = fwd.provenance(_run(tmp_path, [PFX_WRITE, SNAP0, FIX0, SNAP1], prefix=True))
    assert info["label"] == "prefix-tree"
    assert "before the first complete_fix_dispatched" in info["reason"]


def test_prefix_tree_written_after_the_first_fix_dispatch_is_refused(tmp_path):
    # VA-043 (D7 refuter edge): a RESUME into REPAIR of a pre-prefix-tree run writes the dir from a tree the
    # waves already touched -- the write event lands AFTER the run's first complete_fix_dispatched.
    late = dict(PFX_WRITE, ts="2026-09-01T02:00:00.000000+00:00")
    info = fwd.provenance(_run(tmp_path, [SNAP0, FIX0, SNAP1, late], prefix=True))
    assert info["source"] is None
    assert "AFTER the first complete_fix_dispatched" in info["reason"]
    assert info["prefix_tree_written_ts"] == late["ts"] and info["first_fix_dispatched_ts"] == FIX0["ts"]


def test_prefix_tree_dir_without_its_write_event_is_refused(tmp_path):
    info = fwd.provenance(_run(tmp_path, [SNAP0, FIX0], prefix=True))
    assert info["source"] is None and "no prefix_tree_snapshot WRITE event" in info["reason"]


def test_a_skipped_prefix_snapshot_is_not_a_write(tmp_path):
    skipped = dict(PFX_WRITE, skipped="already present", ts="2026-09-01T02:00:00.000000+00:00")
    info = fwd.provenance(_run(tmp_path, [SNAP0, FIX0, skipped], prefix=True))
    assert info["source"] is None and "no prefix_tree_snapshot WRITE event" in info["reason"]


def test_prefix_tree_with_no_wave_dispatched_is_proven(tmp_path):
    info = fwd.provenance(_run(tmp_path, [PFX_WRITE], prefix=True))
    assert info["label"] == "prefix-tree" and "no wave ever ran" in info["reason"]


def test_no_snapshot_event_is_refused(tmp_path):
    info = fwd.provenance(_run(tmp_path, [{"event": "phase", "phase": "repair"}]))
    assert info["source"] is None and "no successful best_tree_snapshot" in info["reason"]


def test_a_failed_snapshot_does_not_count(tmp_path):
    assert fwd.provenance(_run(tmp_path, [dict(SNAP0, ok=False)]))["source"] is None


def test_round0_event_without_the_dir_is_refused(tmp_path):
    info = fwd.provenance(_run(tmp_path, [SNAP0], best=False))
    assert info["source"] is None and "not in the archive" in info["reason"]


def test_identical_ignores_debris_and_sees_app_bytes(tmp_path):
    a, b = tmp_path / "a", tmp_path / "b"
    for r in (a, b):
        (r / "app").mkdir(parents=True)
        (r / "app/x.py").write_text("same\n")
    (a / "engine-console.log").write_text("noise")
    (a / "trace.jsonl").write_text("{}\n")
    (b / "verdict-hermetic-x.json").write_text("{}")
    (b / "app/__pycache__").mkdir()
    (b / "app/__pycache__/x.pyc").write_bytes(b"\x00")
    (a / "graded-sb7-db").mkdir()
    (a / "graded-sb7-db/ledger.db").write_bytes(b"sqlite")
    assert fwd.identical(a, b) == (True, [])
    (b / "app/x.py").write_text("changed\n")
    assert fwd.identical(a, b) == (False, ["app/x.py"])


def test_find_verdict_reads_the_seed_and_kind_never_the_name_alone(tmp_path):
    run = tmp_path / "run"
    run.mkdir()
    (run / "verdict-hermetic-seedd32de873-port8850-0.1420.json").write_text(json.dumps(_v(SEED, 0.72, 0.216, 0.142, [])))
    (run / f"verdict-hermetic-prefix-{SEED}-0.1392.json").write_text(json.dumps(_v(SEED, 0.71, 0.216, 0.139, [])))
    (run / f"verdict-hermetic-snapshot-best-tree-{SEED}-0.1420.json").write_text(json.dumps(_v(SEED, 0.72, 0.216, 0.142, [])))
    (run / "verdict-hermetic-seed00000000-port8850-0.5.json").write_text(json.dumps(_v("0" * 16, 0.9, 1, 0.5, [])))
    assert fwd.find_verdict(run, SEED, "final").name == "verdict-hermetic-seedd32de873-port8850-0.1420.json"
    assert fwd.find_verdict(run, SEED, "prefix").name == f"verdict-hermetic-prefix-{SEED}-0.1392.json"
    assert fwd.find_verdict(run, "1" * 16, "final") is None
