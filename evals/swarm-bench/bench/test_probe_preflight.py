"""The scorer must refuse to grade when the browser probe cannot run under the configured node."""
import importlib.util, os, pathlib, shutil, sys
import pytest

HERE = pathlib.Path(__file__).parent
spec = importlib.util.spec_from_file_location("score_sb7", HERE / "score_sb7.py")
mod = importlib.util.module_from_spec(spec)
sys.modules["score_sb7"] = mod
spec.loader.exec_module(mod)

NVM_NODE = pathlib.Path.home() / ".nvm/versions/node/v22.22.0/bin/node"
HERMIT_NODE = pathlib.Path.home() / "Projects/goose/bin/node"


def test_a_node_that_cannot_run_refuses(monkeypatch):
    monkeypatch.setenv("GOOSE_SWARM_RENDER_NODE", "/usr/bin/false")
    why = mod._probe_preflight()
    assert why and "/usr/bin/false" in why


@pytest.mark.skipif(not HERMIT_NODE.exists(), reason="hermit node not on this machine")
def test_a_node_without_playwright_refuses_and_names_itself(monkeypatch):
    monkeypatch.setenv("GOOSE_SWARM_RENDER_NODE", str(HERMIT_NODE))
    why = mod._probe_preflight()
    if why is None:
        pytest.skip("this hermit node can resolve playwright; the trap is not reproducible here")
    assert "playwright" in why.lower()


@pytest.mark.skipif(not NVM_NODE.exists(), reason="nvm node v22 not on this machine")
def test_the_node_with_playwright_passes(monkeypatch):
    monkeypatch.setenv("GOOSE_SWARM_RENDER_NODE", str(NVM_NODE))
    assert mod._probe_preflight() is None
