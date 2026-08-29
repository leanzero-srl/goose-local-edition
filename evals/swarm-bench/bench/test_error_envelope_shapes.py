"""A string error body must score, not crash -- at every site, through one predicate."""
import importlib.util, pathlib, sys

HERE = pathlib.Path(__file__).parent
spec = importlib.util.spec_from_file_location("score_sb7", HERE / "score_sb7.py")
mod = importlib.util.module_from_spec(spec)
sys.modules["score_sb7"] = mod
spec.loader.exec_module(mod)


def test_string_error_is_not_an_envelope():
    assert mod._error_obj({"error": "Not found"}) == {}


def test_missing_or_non_dict_bodies_are_empty():
    for body in (None, "", [], "Not found", {"data": []}, {"error": None}, {"error": ["x"]}):
        assert mod._error_obj(body) == {}, body


def test_a_real_envelope_comes_back_whole():
    env = {"code": "bad_request", "message": "limit must be positive",
           "field_errors": [{"path": "limit", "code": "range"}]}
    assert mod._error_obj({"error": env}) is env


def test_no_site_reaches_into_error_without_the_predicate():
    src = (HERE / "score_sb7.py").read_text()
    # Scan the code AFTER the predicate's own body -- its docstring quotes the pattern it forbids.
    body = src.split("return err if isinstance(err, dict) else {}", 1)[1]
    for bad in ('(body.get("error") or {})', '(body_bad or {}).get("error")', '.get("error") or {}'):
        assert bad not in body, f"a site bypasses _error_obj: {bad}"
