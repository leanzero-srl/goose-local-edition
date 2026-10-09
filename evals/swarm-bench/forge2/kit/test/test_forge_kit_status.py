"""forge_kit.status(): the desktop's "is the Forge kit ready" answer, read without the network.

Run: python3 -m unittest forge2/kit/test/test_forge_kit_status.py
"""
from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent.parent.parent / 'bench'))
import forge2_kit as forge_kit  # noqa: E402


class StatusTest(unittest.TestCase):
    def test_an_empty_cache_names_every_part_ensure_would_fetch_and_writes_nothing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / 'kit-cache'
            got = forge_kit.status(root)
            self.assertFalse(got['ready'])
            self.assertEqual(got['missing'], ['app-modules', 'lint-modules', 'wrapper/wrapper.js',
                                              'wrapper/loader.js', 'schema', 'kit'])
            self.assertEqual(got['kit_lock_sha256'], forge_kit.lock_sha256())
            self.assertFalse(root.exists())

    def test_a_wrapper_whose_bytes_differ_from_the_pin_is_missing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            modules = root / forge_kit.lock_sha256()[:16]
            for tree in ('app-modules', 'lint-modules'):
                (modules / tree).mkdir(parents=True)
                (modules / tree / '.complete').write_text('x')
            (modules / 'wrapper').mkdir()
            (modules / 'wrapper' / 'wrapper.js').write_text('not the pinned runtime')
            (modules / 'wrapper' / 'loader.js').write_text('not the pinned loader')
            got = forge_kit.status(root)
            self.assertIn('wrapper/wrapper.js', got['missing'])
            self.assertIn('wrapper/loader.js', got['missing'])
            self.assertNotIn('app-modules', got['missing'])


if __name__ == '__main__':
    unittest.main()
