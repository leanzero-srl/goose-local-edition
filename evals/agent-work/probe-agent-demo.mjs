// Run a disposable Agent Work directory through the same MLX mount API as the desktop.
// Arguments: isolated Goose configuration root, agent directory, local model ID.
import { spawn } from 'node:child_process';
import { Readable, Writable } from 'node:stream';
import { readFile, readdir, mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { GooseClient } from '../../ui/sdk/dist/goose-client.js';
import { ndJsonStream } from '../../ui/node_modules/@agentclientprotocol/sdk/dist/acp.js';
const [root, agentDir, modelId] = process.argv.slice(2);
if (!root || !agentDir || !modelId) throw new Error('Supply an isolated configuration root, demo directory and model ID.');
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const env = { ...process.env, GOOSE_PATH_ROOT: root, GOOSE_DISABLE_KEYRING: '1' };
// The demo's shell must not inherit account tokens, CLI profiles or SSH agents.
// This is environment isolation, not a filesystem sandbox.
const demoHome = path.join(root, 'demo-home');
await mkdir(demoHome, { recursive: true });
const demoEnv = Object.fromEntries(Object.entries(env).filter(([name]) =>
  !/TOKEN|KEY|SECRET|PASS|AUTH/i.test(name)
));
Object.assign(demoEnv, {
  HOME: demoHome, XDG_CONFIG_HOME: path.join(demoHome, '.config'),
  GH_CONFIG_DIR: path.join(demoHome, '.config/gh'), GIT_CONFIG_GLOBAL: '/dev/null',
  GOOSE_DISABLE_KEYRING: '1',
});
const binary = path.join(repo, 'target/debug/goose');
const backend = spawn(binary, ['acp'], { env, stdio: ['pipe', 'pipe', 'ignore'] });
const client = new GooseClient(() => ({ sessionUpdate() {}, requestPermission: async () => ({ outcome: { outcome: 'cancelled' } }) }), ndJsonStream(Writable.toWeb(backend.stdin), Readable.toWeb(backend.stdout)));
try {
  await client.initialize({ protocolVersion: 1, clientCapabilities: {}, clientInfo: { name: 'agent-demo-acceptance', version: '1' } });
  await client.goose.mlxEngineMount_unstable({ modelId });
  let status;
  do {
    status = (await client.goose.mlxEngineStatus_unstable({})).status;
    if (status.state === 'failed') throw new Error(JSON.stringify(status));
    if (status.state !== 'running') await new Promise((resolve) => setTimeout(resolve, 1000));
  } while (status.state !== 'running');
  assert.ok(status.toolCallParser, 'The actual mounted engine must report a tool parser.');
  console.log(JSON.stringify({ mounted: true, parser: status.toolCallParser, pid: status.pid }));
  const run = spawn(binary, ['swarm', 'agent', 'run', agentDir, '--once'], { env: demoEnv, stdio: ['ignore', 'inherit', 'inherit'] });
  const code = await new Promise((resolve, reject) => { run.once('error', reject); run.once('exit', resolve); });
  assert.equal(code, 0);
  const events = (await readFile(path.join(agentDir, '.swarm/agent/run.jsonl'), 'utf8')).trim().split('\n').map(JSON.parse);
  assert.ok(events.some((event) => event.event === 'tick_done'));
  assert.ok(events.some((event) => event.event === 'agent_stopped'));
  assert.ok(!events.some((event) => /(?:_failed|_unparseable)$/.test(event.event)), 'A failed lane or synthesis is not a completed demo.');
  const synthesis = events.find((event) => event.event === 'synthesis_done');
  assert.ok(synthesis, 'The run must deliver a synthesis.');
  const activity = path.join(agentDir, '.swarm/activity');
  const calls = (await Promise.all((await readdir(activity)).filter((file) => file.endsWith('.calls.jsonl')).map(async (file) =>
    (await readFile(path.join(activity, file), 'utf8')).trim().split('\n').filter(Boolean).map(JSON.parse)
  ))).flat();
  assert.ok(calls.some((call) => call.name.endsWith('__get-single-web-page-content') && call.ok), 'The demo must actually read a page through its selected MCP.');
  const receipt = { passed: true, agentDir, parser: status.toolCallParser, events: events.length };
  await writeFile(path.join(agentDir, 'acceptance.json'), JSON.stringify(receipt, null, 2));
  console.log(JSON.stringify(receipt));
} finally {
  await client.goose.mlxEngineUnmount_unstable({});
  backend.stdin.end();
  backend.kill('SIGTERM');
}
