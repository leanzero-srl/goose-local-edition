// Uses disposable customer tokens supplied by the operator, never an admin key.
// No email provisioning or changes to the user's Goose profile.
import { spawn } from 'node:child_process';
import { Readable, Writable } from 'node:stream';
import { mkdtemp, readFile, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { GooseClient } from '../../ui/sdk/dist/goose-client.js';
import { ndJsonStream } from '../../ui/node_modules/@agentclientprotocol/sdk/dist/acp.js';
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
assert.ok(process.env.MCP_TEST_TENANTS_FILE, 'Provide a private file containing disposable test tenant tokens');
const tenants = JSON.parse(await readFile(process.env.MCP_TEST_TENANTS_FILE, 'utf8'));
const root = await mkdtemp(path.join(tmpdir(), 'goose-hosted-setup-'));
await mkdir(path.join(root, 'config'));
const child = spawn(path.join(repo, 'target/debug/goose'), ['acp'], {
  env: { PATH: process.env.PATH, HOME: root, GOOSE_PATH_ROOT: root, GOOSE_DISABLE_KEYRING: '1' },
  stdio: ['pipe', 'pipe', 'ignore'],
});
const client = new GooseClient(() => ({ sessionUpdate() {}, requestPermission: async () => ({ outcome: { outcome: 'cancelled' } }) }), ndJsonStream(Writable.toWeb(child.stdin), Readable.toWeb(child.stdout)));
try {
  await client.initialize({ protocolVersion: 1, clientCapabilities: {}, clientInfo: { name: 'fresh-customer-setup', version: '1' } });
  const receipts = [];
  for (const [server, name, service, secret] of [
    ['mcp-web-search', 'LeanZero Web Search', 'websearch', 'LEANZERO_HOSTED_WEB_ACCESS_TOKEN'],
    ['mcp-doc-processor', 'LeanZero Documents', 'docproc', 'LEANZERO_HOSTED_DOCUMENTS_ACCESS_TOKEN'],
  ]) {
    const token = tenants[server][0].bearer;
    const uri = `https://worksmacstudio.tailfc4700.ts.net/${service}/mcp`;
    const extension = { type: 'mcp', server: { type: 'http', name, url: uri, headers: [{ name: 'Authorization', value: `Bearer \${${secret}}` }] }, envKeys: [secret], timeout: 300 };
    await client.goose.configUpsert_unstable({ key: secret, value: 'invalid-customer-token', isSecret: true });
    await client.goose.configExtensionsAdd_unstable({ enabled: false, extension });
    await assert.rejects(client.goose.configExtensionsInspect_unstable({ name }), 'Invalid token must fail connection');
    await client.goose.configUpsert_unstable({ key: secret, value: token, isSecret: true });
    const result = await client.goose.configExtensionsInspect_unstable({ name });
    assert.ok(result.tools.length > 0);
    const listed = await client.goose.configExtensionsList_unstable({});
    assert.ok(!JSON.stringify(listed).includes(token));
    const saved = listed.extensions.find((x) => x.extension.server?.name === name);
    assert.equal(saved.extension.server.type, 'http');
    assert.equal(saved.extension.server.url, uri);
    const publicConfig = await readFile(path.join(root, 'config', 'config.yaml'), 'utf8');
    assert.ok(!publicConfig.includes(token));
    receipts.push({ name, tools: result.tools.length, invalidTokenRejected: true, transportReloaded: true, secretProtected: true });
  }
  console.log(JSON.stringify({ passed: true, isolatedRoot: root, checks: receipts }));
} finally {
  child.stdin.end();
  child.kill('SIGTERM');
}
