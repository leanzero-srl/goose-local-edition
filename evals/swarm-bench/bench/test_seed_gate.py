"""The scorer must refuse to draw a seed silently: a tree is scored against the seed its run was fed."""
import pathlib, subprocess, sys

HERE = pathlib.Path(__file__).parent


def _run(*extra):
    tree = HERE / "_seed_gate_tree"
    tree.mkdir(exist_ok=True)
    return subprocess.run([sys.executable, str(HERE / "score_sb7.py"), "--tree", str(tree), *extra],
                          capture_output=True, text=True, timeout=180)


def test_no_seed_is_refused_before_anything_starts():
    r = _run()
    assert r.returncode == 2, (r.returncode, r.stderr[-600:])
    assert "REFUSED: no --seed" in r.stderr and "fixture_seed" in r.stderr


def test_a_malformed_seed_is_still_refused():
    r = _run("--seed", "nope")
    assert r.returncode == 2 and "16 hex chars" in r.stderr, r.stderr[-400:]
