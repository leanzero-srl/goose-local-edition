/**
 * Q-138 — a bundled stdio MCP ends with its client.
 *
 * Measured 2026-09-26: eight leanzero-web-search processes outlived the goose that spawned them
 * (PPID 1), six at 100% CPU for up to 3.4 days, all deaf to SIGTERM — an EPIPE → uncaughtException
 * → console.error → EPIPE loop on the dead stderr pipe. These tests run the BUILT bundle (the patched
 * sources from scripts/mcp-patches, produced by `node scripts/bundle-mcps.mjs`) the way goose does:
 * `node <entry>` over three pipes, one initialize round-trip, then the client goes away.
 *
 * Run: pnpm run test:integration tests/integration/bundled_mcp_lifeline.test.ts
 */
import { describe, expect, it } from 'vitest';
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const bundledRoot = path.resolve(__dirname, '..', '..', 'bundled-mcps');
const pins = JSON.parse(
  fs.readFileSync(path.resolve(__dirname, '..', '..', 'scripts', 'bundled-mcps.json'), 'utf8')
) as { id: string; entry: string }[];

function entryOf(id: string): string {
  const pin = pins.find((p) => p.id === id);
  if (!pin) throw new Error(`${id} is not in scripts/bundled-mcps.json`);
  const entry = path.join(bundledRoot, pin.id, pin.entry);
  if (!fs.existsSync(entry)) {
    throw new Error(`${entry} is missing — build the bundle first: node scripts/bundle-mcps.mjs`);
  }
  return entry;
}

type Exit = { code: number | null; signal: NodeJS.Signals | null; afterMs: number };

async function startInitialized(id: string): Promise<{
  child: ChildProcessWithoutNullStreams;
  exited: (since: number) => Promise<Exit>;
}> {
  const child = spawn(process.execPath, [entryOf(id)], {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, MCP_CLIENT_TYPE: 'agent' },
  });
  const exit = new Promise<{ code: number | null; signal: NodeJS.Signals | null; at: number }>(
    (resolve) => child.once('exit', (code, signal) => resolve({ code, signal, at: Date.now() }))
  );
  child.stderr.resume();
  const reply = new Promise<string>((resolve, reject) => {
    let buffered = '';
    child.stdout.on('data', (chunk: Buffer) => {
      buffered += chunk.toString('utf8');
      const newline = buffered.indexOf('\n');
      if (newline >= 0) resolve(buffered.slice(0, newline));
    });
    child.once('exit', (code) =>
      reject(new Error(`${id} exited (${code}) before answering initialize`))
    );
  });
  child.stdin.write(
    JSON.stringify({
      jsonrpc: '2.0',
      id: 1,
      method: 'initialize',
      params: {
        protocolVersion: '2025-03-26',
        capabilities: {},
        clientInfo: { name: 'q138-lifeline', version: '0' },
      },
    }) + '\n'
  );
  expect(JSON.parse(await reply)).toMatchObject({ id: 1, result: expect.any(Object) });
  return {
    child,
    exited: async (since) => {
      const { code, signal, at } = await exit;
      return { code, signal, afterMs: at - since };
    },
  };
}

describe('Q-138: a bundled stdio MCP ends with its client', () => {
  for (const id of ['leanzero-web-search', 'leanzero-documents']) {
    it(`${id} exits on its own when stdin closes (the client is gone)`, async () => {
      const { child, exited } = await startInitialized(id);
      const since = Date.now();
      child.stdin.end();
      const exit = await exited(since);
      console.log(`${id}: stdin closed → exited code=${exit.code} after ${exit.afterMs} ms`);
      expect(exit.signal).toBeNull();
      expect(exit.code).toBe(0);
    });
  }

  it('leanzero-web-search exits when its stdout reader is gone (EPIPE), instead of looping on it', async () => {
    const { child, exited } = await startInitialized('leanzero-web-search');
    child.stdout.destroy();
    child.stderr.destroy();
    const since = Date.now();
    // stdin stays open: the only way out is the write that hits the dead pipe.
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: 2, method: 'tools/list' }) + '\n');
    const exit = await exited(since);
    console.log(
      `leanzero-web-search: dead stdout → exited code=${exit.code} after ${exit.afterMs} ms`
    );
    expect(exit.signal).toBeNull();
    expect(exit.code).toBe(0);
    child.stdin.destroy();
  });

  it('leanzero-web-search honours SIGTERM while connected (no SIGKILL needed)', async () => {
    const { child, exited } = await startInitialized('leanzero-web-search');
    const since = Date.now();
    child.kill('SIGTERM');
    const exit = await exited(since);
    console.log(`leanzero-web-search: SIGTERM → exited code=${exit.code} after ${exit.afterMs} ms`);
    // The server's own handler exits 0; a process that ignored the signal never gets here.
    expect(exit.code).toBe(0);
    child.stdin.destroy();
  });
});
