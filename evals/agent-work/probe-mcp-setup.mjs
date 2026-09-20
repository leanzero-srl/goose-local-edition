// Real ACP + bundled MCP acceptance, isolated from the user's configuration.
import { spawn } from 'node:child_process';
import { Readable, Writable } from 'node:stream';
import { mkdtemp, readFile, readdir, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { GooseClient } from '../../ui/sdk/dist/goose-client.js';
import { ndJsonStream } from '../../ui/node_modules/@agentclientprotocol/sdk/dist/acp.js';
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const root = await mkdtemp(path.join(tmpdir(), 'goose-mcp-setup-'));
await mkdir(path.join(root, 'config'));
const child = spawn(path.join(repo, 'target/debug/goose'), ['acp'], {
  env: { ...process.env, GOOSE_PATH_ROOT: root, GOOSE_DISABLE_KEYRING: '1' },
  stdio: ['pipe', 'pipe', 'ignore'],
});
const client = new GooseClient(() => ({ sessionUpdate() {}, requestPermission: async () => ({ outcome: { outcome: 'cancelled' } }) }), ndJsonStream(Writable.toWeb(child.stdin), Readable.toWeb(child.stdout)));
try {
  await client.initialize({ protocolVersion: 1, clientCapabilities: {}, clientInfo: { name: 'mcp-setup-acceptance', version: '1' } });
  const bundle = path.join(repo, 'ui/desktop/bundled-mcps');
  const browser = JSON.parse(await readFile(path.join(bundle, 'browser.json'), 'utf8'));
  const env = {
    OUTPUT_DIR: path.join(root, 'corpus'),
    PUPPETEER_EXECUTABLE_PATH: path.join(bundle, browser.executable),
    LEANZERO_BROWSER_EXECUTABLE: path.join(bundle, browser.executable),
    SERPER_API_KEY: 'fixture-not-a-real-key',
  };
  await client.goose.configExtensionsAdd_unstable({ enabled: false, extension: { type: 'mcp', server: {
    name: 'LeanZero Web Search', command: process.execPath,
    args: [path.join(bundle, 'leanzero-web-search/dist/index.js')],
    env: Object.entries(env).map(([name, value]) => ({ name, value })),
  }, timeout: 300 } });
  const settings = await client.goose.configExtensionsInspect_unstable({ name: 'LeanZero Web Search', settingsOnly: true });
  assert.equal(settings.settings.OUTPUT_DIR, env.OUTPUT_DIR);
  assert.ok(!JSON.stringify(settings).includes(env.SERPER_API_KEY));
  const list = await client.goose.configExtensionsList_unstable({});
  const saved = list.extensions.find((entry) => entry.extension.type === 'mcp' && entry.extension.server.name === 'LeanZero Web Search');
  assert.ok(saved.extension.envKeys.includes('SERPER_API_KEY'));
  assert.ok(!JSON.stringify(saved).includes(env.SERPER_API_KEY));
  const inspected = await client.goose.configExtensionsInspect_unstable({ name: 'LeanZero Web Search' });
  assert.ok(inspected.tools.some((tool) => tool.name === 'get-single-web-page-content'));
  const collected = await client.goose.configExtensionsInspect_unstable({ name: 'LeanZero Web Search', sourceUrl: 'https://developer.mozilla.org/en-US/docs/Web/JavaScript' });
  assert.ok(collected.savedFile.startsWith(env.OUTPUT_DIR + path.sep));
  const body = await readFile(collected.savedFile, 'utf8');
  assert.match(body, /JavaScript/);
  assert.match(body, /Source: https:\/\/developer.mozilla.org/);
  assert.match(body, /Collected:/);
  const beforeInvalidSource = await readdir(env.OUTPUT_DIR);
  await assert.rejects(client.goose.configExtensionsInspect_unstable({ name: 'LeanZero Web Search', sourceUrl: 'file:///etc/passwd' }));
  assert.deepEqual(await readdir(env.OUTPUT_DIR), beforeInvalidSource);
  await client.goose.configExtensionsAdd_unstable({ enabled: false, extension: { type: 'mcp', server: {
    name: 'LeanZero Documents', command: process.execPath,
    args: [path.join(bundle, 'leanzero-documents/src/index.js')],
    env: [{ name: 'DOC_OUTPUT_DIR', value: path.join(root, 'documents') }],
  }, timeout: 300 } });
  const documents = await client.goose.configExtensionsInspect_unstable({ name: 'LeanZero Documents' });
  assert.equal(documents.settings.DOC_OUTPUT_DIR, path.join(root, 'documents'));
  assert.ok(documents.tools.some((tool) => /pdf/i.test(tool.name)));
  console.log(JSON.stringify({ passed: true, webTools: inspected.tools.length, documentTools: documents.tools.length, source: collected.savedFile, isolatedRoot: root }));
} finally {
  child.stdin.end();
  child.kill('SIGTERM');
}
