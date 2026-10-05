"""The entrant's workspace for forge-1.0, measured end to end (DESIGN.md §4, §6.5):

bench_isolation.prepare(network='fenced', extra_read=[kit]) builds the sandbox; inside it the internet is
unreachable (directly and through the relay), the provider answers through the relay, localhost works,
and the dev kit runs: `npm run lint` (bin/lint.cjs) offline and forge-dev against a dev site started
outside the sandbox (forge_site.serve). Run: python3 -m unittest forge/kit/test/test_entrant_fence.py
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent.parent.parent / 'bench'
sys.path.insert(0, str(BENCH))
import bench_isolation  # noqa: E402
import forge_kit  # noqa: E402
import forge_site  # noqa: E402

MANIFEST = """modules:
  scheduledTrigger:
    - key: hourly
      function: sweep
      interval: hour
  function:
    - key: sweep
      handler: index.sweep
permissions:
  scopes:
    - read:jira-work
    - storage:app
app:
  id: ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000
  runtime:
    name: nodejs22.x
"""
SOURCE = """import api, { route } from '@forge/api';
import { kvs } from '@forge/kvs';
export const sweep = async () => {
  const res = await api.asApp().requestJira(route`/rest/api/3/search/jql`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ jql: 'sprint in openSprints()', fields: ['summary'], maxResults: 5 }),
  });
  const page = await res.json();
  await kvs.set('last', page.issues.length);
  return { status: res.status, issues: page.issues.length };
};
"""


@unittest.skipUnless(sys.platform == 'darwin', 'macOS sandbox integration')
class EntrantFenceTests(unittest.TestCase):
    def test_fenced_workspace_runs_the_dev_kit_and_reaches_only_the_provider(self):
        kit = forge_kit.ensure()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            work, private = root / 'candidate', root / 'private'
            work.mkdir()
            private.mkdir()
            (private / 'reference.json').write_text('{"secret": true}')
            (work / 'manifest.yml').write_text(MANIFEST)
            (work / 'src').mkdir()
            (work / 'src' / 'index.js').write_text(SOURCE)
            forge_kit.install_node_modules(work, kit)
            snapshot = {'version': 1, 'providers': ['openrouter'], 'config': {}, 'secrets': {}, 'custom_providers': {}}
            prefix, env = bench_isolation.prepare(work, Path(sys.executable), private, snapshot=snapshot,
                                                  network='fenced', extra_read=[kit['kit_dir']])
            try:
                isolation = json.loads((work / 'isolation.json').read_text())
                self.assertEqual(isolation['network'], 'fenced')
                self.assertEqual(isolation['relay']['allow'], ['openrouter.ai:443'])
                self.assertIn('blocked', isolation['preflight']['direct_internet'])
                self.assertIn('refused', isolation['preflight']['relay_non_provider'])
                self.assertIn('answered HTTP', isolation['preflight']['relay_provider'])
                full_env = {**os.environ, **env, 'FORGE_KIT': kit['kit_dir']}
                node = str(bench_isolation.node_runtime())

                def run(*args, cwd=work, extra=None):
                    return subprocess.run(prefix + list(args), cwd=cwd, env={**full_env, **(extra or {})},
                                          capture_output=True, text=True, timeout=300)

                probe = run(node, '-e', """
                    const http = require('http');
                    (async () => {
                      const out = {};
                      try { await fetch('https://registry.npmjs.org/@forge%2fapi'); out.internet = 'reached'; } catch (e) { out.internet = String(e.cause?.code ?? e.message); }
                      const srv = http.createServer((q, s) => s.end('ok')).listen(0, '127.0.0.1');
                      await new Promise((r) => srv.on('listening', r));
                      const r = await fetch(`http://127.0.0.1:${srv.address().port}/`);
                      out.localhost = r.status;
                      try { require('fs').readFileSync(process.argv[1]); out.private = 'read'; } catch (e) { out.private = e.code; }
                      srv.close();
                      console.log(JSON.stringify(out));
                    })();""", str(private / 'reference.json'))
                self.assertEqual(probe.returncode, 0, probe.stderr)
                facts = json.loads(probe.stdout.strip().splitlines()[-1])
                self.assertNotEqual(facts['internet'], 'reached')
                self.assertEqual(facts['localhost'], 200)
                self.assertEqual(facts['private'], 'EPERM')
                self.assertEqual(env['HTTPS_PROXY'], isolation['relay']['url'])

                lint = run(node, str(Path(kit['kit_dir']) / 'bin' / 'lint.cjs'), '--json')
                self.assertIn(lint.returncode, (0, 1), lint.stderr[-2000:])
                result = json.loads(lint.stdout.split('LINT_JSON ', 1)[1])
                self.assertEqual(result['counts']['errors'], 0, result['problems'])
                self.assertEqual(result['stageReached'], 'complete')

                site = forge_site.serve(0, root / 'trace.jsonl', seed='0123456789abcdef')
                try:
                    dev = run(node, str(Path(kit['kit_dir']) / 'bin' / 'forge-dev.cjs'), 'scheduled', 'hourly',
                              extra={'FORGE_SITE_URL': site.url})
                    self.assertEqual(dev.returncode, 0, dev.stdout[-2000:] + dev.stderr[-2000:])
                    self.assertIn('fence: node --permission', dev.stdout)
                    self.assertIn('"status": 200', dev.stdout)
                    self.assertIn('jira   app  POST /rest/api/3/search/jql -> 200', dev.stdout)
                    kvs = run(node, str(Path(kit['kit_dir']) / 'bin' / 'forge-dev.cjs'), 'kvs')
                    self.assertEqual(json.loads(kvs.stdout)['kvs'], {'last': 5})
                    header = forge_site.trace_header(root / 'trace.jsonl')
                    self.assertEqual(header['fixture_seed'], '0123456789abcdef')
                    self.assertNotEqual(header['dev_seed'], header['fixture_seed'])
                    self.assertEqual(header['kit_lock_sha256'], kit['kit_lock_sha256'])
                finally:
                    site.shutdown()
                    site.server_close()
            finally:
                bench_isolation.stop_relays()

    def test_relay_refuses_every_host_but_the_allowlist(self):
        relay = bench_isolation.ProviderRelay(['openrouter.ai:443'])
        try:
            import socket
            with socket.create_connection(('127.0.0.1', relay.port)) as s:
                s.sendall(b'CONNECT developer.atlassian.com:443 HTTP/1.1\r\nHost: developer.atlassian.com:443\r\n\r\n')
                self.assertTrue(s.recv(100).startswith(b'HTTP/1.1 403'))
            with socket.create_connection(('127.0.0.1', relay.port)) as s:
                s.sendall(b'GET http://openrouter.ai/ HTTP/1.1\r\nHost: openrouter.ai\r\n\r\n')
                self.assertTrue(s.recv(100).startswith(b'HTTP/1.1 403'))
            self.assertEqual([a['allowed'] for a in relay.attempts], [False, False])
        finally:
            relay.stop()

    def test_provider_hosts_follow_the_snapshot_and_refuse_the_unknown(self):
        snap = {'providers': ['anthropic'], 'config': {'ANTHROPIC_HOST': 'https://proxy.example.com:8443'}, 'secrets': {}, 'custom_providers': {}}
        self.assertEqual(bench_isolation.provider_hosts(snap), ['proxy.example.com:8443'])
        snap = {'providers': ['openrouter', 'lmstudio'], 'config': {}, 'secrets': {}, 'custom_providers': {}}
        self.assertEqual(bench_isolation.provider_hosts(snap), ['openrouter.ai:443'])
        snap = {'providers': ['mycloud'], 'config': {}, 'secrets': {}, 'custom_providers': {'mycloud': {'base_url': 'https://llm.example.org/v1'}}}
        self.assertEqual(bench_isolation.provider_hosts(snap), ['llm.example.org:443'])
        with self.assertRaisesRegex(RuntimeError, 'REFUSED'):
            bench_isolation.provider_hosts({'providers': ['unheard-of'], 'config': {}, 'secrets': {}, 'custom_providers': {}})
        with self.assertRaisesRegex(RuntimeError, 'REFUSED'):
            bench_isolation.provider_hosts({'providers': ['lmstudio'], 'config': {}, 'secrets': {}, 'custom_providers': {}})

    def test_a_loopback_provider_is_reached_directly_not_relayed(self):
        # 2026-10-05: our fine-tune on the LeanZero MLX engine (http://127.0.0.1:8096 via the OpenAI provider)
        snap = {'providers': ['openai'], 'config': {'OPENAI_HOST': 'http://127.0.0.1:8096'}, 'secrets': {}, 'custom_providers': {}}
        self.assertEqual(bench_isolation.provider_hosts(snap), [])
        snap = {'providers': ['openai', 'openrouter'], 'config': {'OPENAI_HOST': 'http://localhost:8096'}, 'secrets': {}, 'custom_providers': {}}
        self.assertEqual(bench_isolation.provider_hosts(snap), ['openrouter.ai:443'])
        snap = {'providers': ['mine'], 'config': {}, 'secrets': {}, 'custom_providers': {'mine': {'base_url': 'http://127.0.0.1:9000/v1'}}}
        self.assertEqual(bench_isolation.provider_hosts(snap), [])
        # controls: a plain-http REMOTE endpoint is still refused, and so is an unset default (https api.openai.com is relayed)
        with self.assertRaisesRegex(RuntimeError, 'HTTPS provider endpoints only'):
            bench_isolation.provider_hosts({'providers': ['openai'], 'config': {'OPENAI_HOST': 'http://llm.example.org'}, 'secrets': {}, 'custom_providers': {}})
        self.assertEqual(bench_isolation.provider_hosts({'providers': ['openai'], 'config': {}, 'secrets': {}, 'custom_providers': {}}), ['api.openai.com:443'])

    def test_default_provider_hosts_match_the_rust_providers(self):
        providers = HERE.parents[3].parent / 'crates' / 'goose' / 'src' / 'providers'
        if not providers.is_dir():
            self.skipTest('goose source tree not present')
        source = '\n'.join(p.read_text() for p in providers.glob('*.rs'))
        for name, (key, url) in bench_isolation.PROVIDER_DEFAULT_HOSTS.items():
            self.assertIn(f'"{url}"', source, f'{name}: default {url} no longer in crates/goose/src/providers')
            if key:
                self.assertIn(f'"{key}"', source, f'{name}: config key {key} no longer in crates/goose/src/providers')

    def test_open_network_profile_is_unchanged(self):
        args = (Path('/tmp/w'), Path('/usr/bin/true'), Path('/tmp/r'), Path('/usr/bin/true'), None)
        text = bench_isolation.profile(*args)
        self.assertIn('(allow network*)', text)
        self.assertNotIn('remote ip "localhost:*"', text)
        fenced = bench_isolation.profile(*args, network='fenced')
        self.assertNotIn('(allow network*)', fenced)
        self.assertEqual(set(fenced.split('\n')) - set(text.split('\n')), set(bench_isolation.FENCED_RULES))


if __name__ == '__main__':
    unittest.main()
