import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from bench_task_identity import capture, preserve


class LaunchTaskIdentity(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'forge2').mkdir()
        (self.root / 'task.md').write_text('contract before launch')
        self.path = self.root / 'forge2/release-manifest.json'
        self.manifest = {'schemaVersion': 1, 'family': 'forge', 'scorerVersion': 'forge-2.0',
                         'files': {'task.md': hashlib.sha256((self.root / 'task.md').read_bytes()).hexdigest()}}
        self.path.write_text(json.dumps(self.manifest))

    def test_capture_pins_raw_bytes_before_later_changes(self):
        raw = self.path.read_bytes()
        original = capture(self.root, 'build-1')
        self.path.write_text(json.dumps(self.manifest, indent=2))
        later = capture(self.root, 'build-2')
        self.assertEqual(original['taskFingerprint'], hashlib.sha256(raw).hexdigest())
        self.assertNotEqual(original['taskFingerprint'], later['taskFingerprint'])
        self.assertEqual(original['buildId'], 'build-1')

    def test_changed_payload_or_invalid_manifest_cannot_start(self):
        (self.root / 'task.md').write_text('different contract')
        with self.assertRaisesRegex(ValueError, 'installed payload differs'):
            capture(self.root, 'build')
        with self.assertRaisesRegex(ValueError, 'build identity'):
            capture(self.root, None)
        for bad in ('../task.md', '/task.md', 'forge2/release-manifest.json'):
            self.manifest['files'] = {bad: 'a' * 64}
            self.path.write_text(json.dumps(self.manifest))
            with self.subTest(path=bad), self.assertRaisesRegex(ValueError, 'invalid payload path'):
                capture(self.root, 'build')

    def test_scoring_retry_uses_recorded_launch_identity_and_inputs_only(self):
        launch = capture(self.root, 'build')
        launch.update(input_manifest={'task.md': 'before'}, reasoning_effort={'value': 'medium'})
        self.path.write_text('{}')
        result = {'publishable': True, 'taskFingerprint': 'f' * 64, 'buildId': 'manufactured'}
        preserve({'runId': 'build', 'launchIdentity': launch}, result)
        self.assertEqual(result['taskFingerprint'], launch['taskFingerprint'])
        self.assertEqual(result['buildId'], 'build')
        self.assertEqual(result['input_manifest'], {'task.md': 'before'})
        self.assertEqual(result['reasoning_effort'], {'value': 'medium'})

    def test_legacy_receipt_can_be_scored_but_cannot_acquire_current_identity(self):
        result = {'publishable': True, 'taskFingerprint': 'f' * 64, 'buildId': 'manufactured'}
        preserve({'runId': 'old-build'}, result)
        self.assertFalse(result['publishable'])
        self.assertNotIn('taskFingerprint', result)
        self.assertNotIn('buildId', result)
        with self.assertRaisesRegex(ValueError, 'identities differ'):
            preserve({'runId': 'wrong', 'launchIdentity': capture(self.root, 'build')}, {})


if __name__ == '__main__': unittest.main()
