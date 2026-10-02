"""Runs test_sb71_drag_budget_read.mjs in the bundled browser (supply the GOOSE_SWARM_* runtime env)."""
import os
from pathlib import Path
import subprocess
import unittest

BUNDLED = ('GOOSE_SWARM_PLAYWRIGHT_MODULE', 'GOOSE_SWARM_CHROMIUM_EXECUTABLE')


@unittest.skipUnless(all(os.environ.get(name) for name in BUNDLED), 'bundled browser runtime not supplied')
class DragBudgetRead(unittest.TestCase):
    def test_the_post_release_sample_is_waited_for_not_raced(self):
        script = Path(__file__).with_name('test_sb71_drag_budget_read.mjs')
        result = subprocess.run([os.environ.get('GOOSE_SWARM_RENDER_NODE', 'node'), str(script)],
                                capture_output=True, text=True, timeout=240)
        self.assertEqual(result.returncode, 0, result.stderr[-2000:])


if __name__ == '__main__':
    unittest.main()
