"""A held vendor port is named before anything binds, and a free one passes."""
import importlib.util, pathlib, socket, sys

HERE = pathlib.Path(__file__).parent
spec = importlib.util.spec_from_file_location("score_sb7", HERE / "score_sb7.py")
mod = importlib.util.module_from_spec(spec)
sys.modules["score_sb7"] = mod
spec.loader.exec_module(mod)


def test_a_held_port_is_named_and_a_free_one_passes():
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(("127.0.0.1", 0)); s.listen(1)
    port = s.getsockname()[1]
    try:
        why = mod._port_holder(port)
        assert why and f"port {port} is held" in why, why
    finally:
        s.close()
    assert mod._port_holder(port) is None
