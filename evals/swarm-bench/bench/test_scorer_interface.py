"""Every tier scorer exposes every attribute the launcher and the rescorer call on it.

Receipt: 3.0.87's first SB7.2 launch died in run_build.run with
`AttributeError: module 'score_sb72' has no attribute '_draw_seed'` — the tier was validated through
its own scorer CLI, never through run_build, and nothing compared the two interfaces.
"""
import importlib
import re
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

CALLERS = ("run_build.py", "bench_rescore.py")
TIER_SCORERS = ("score_sb7", "score_sb71", "score_sb72")


class ScorerInterfaceTests(unittest.TestCase):
    def test_every_tier_scorer_has_every_attribute_its_callers_use(self):
        used = set()
        for name in CALLERS:
            used |= set(re.findall(r"\bscorer\.([A-Za-z_][A-Za-z0-9_]*)", (HERE / name).read_text()))
        self.assertTrue(used, "no scorer.<attr> uses found — the pattern no longer matches the callers")
        for module_name in TIER_SCORERS:
            module = importlib.import_module(module_name)
            missing = sorted(attr for attr in used if not hasattr(module, attr))
            self.assertEqual(missing, [], f"{module_name} lacks {missing}")


if __name__ == "__main__":
    unittest.main()
