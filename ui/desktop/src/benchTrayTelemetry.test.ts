import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  BenchTelemetryCounter,
  benchTelemetryCandidates,
  readBenchCallBudget,
} from './benchTrayTelemetry';

const MODEL = 'openai/gpt-6.1-sol';
const call = (model = MODEL) => `${JSON.stringify({ model, response_id: 'gen-x' })}\n`;

let root: string;
beforeEach(async () => {
  root = await fs.mkdtemp(path.join(os.tmpdir(), 'bench-tray-'));
});
afterEach(async () => {
  await fs.rm(root, { recursive: true, force: true });
});

describe('the entrant telemetry counter', () => {
  it('reads the isolated runtime sink first, then the tree copy', () => {
    const workdir = path.join(root, 'runs', 'build', 'openrouter-cloud-1-r0');
    expect(benchTelemetryCandidates(workdir)).toEqual([
      path.join(root, 'runs', 'build', '.openrouter-cloud-1-r0-runtime', 'telemetry.jsonl'),
      path.join(workdir, '.swarm', 'telemetry.jsonl'),
    ]);
  });

  it('claims nothing before a sink exists, then counts appended whole lines only', async () => {
    const sink = path.join(root, 'telemetry.jsonl');
    const counter = new BenchTelemetryCounter([sink], MODEL);
    expect(await counter.read()).toBeNull();
    await fs.writeFile(sink, call() + call('openai/gpt-5-nano') + call());
    expect(await counter.read()).toBe(2);
    await fs.appendFile(sink, call() + '{"model": "openai/gpt-6.1-sol"');
    expect(await counter.read()).toBe(3);
    await fs.appendFile(sink, ', "response_id": "gen-y"}\n');
    expect(await counter.read()).toBe(4);
  });

  it('restarts from the first byte when the sink is truncated or replaced', async () => {
    const sink = path.join(root, 'telemetry.jsonl');
    const counter = new BenchTelemetryCounter([sink], MODEL);
    await fs.writeFile(sink, call() + call() + call());
    expect(await counter.read()).toBe(3);
    await fs.writeFile(sink, call());
    expect(await counter.read()).toBe(1);
    const replacement = path.join(root, 'next.jsonl');
    await fs.writeFile(replacement, call() + call() + call() + call() + call());
    await fs.rename(replacement, sink);
    expect(await counter.read()).toBe(5);
  });

  it('moves to the tree copy when the runtime sink is gone', async () => {
    const runtime = path.join(root, 'runtime.jsonl');
    const landed = path.join(root, 'landed.jsonl');
    const counter = new BenchTelemetryCounter([runtime, landed], MODEL);
    await fs.writeFile(runtime, call() + call());
    expect(await counter.read()).toBe(2);
    await fs.writeFile(landed, call() + call() + call());
    await fs.rm(runtime);
    expect(await counter.read()).toBe(3);
  });
});

describe('the call budget read', () => {
  const benchDir = path.resolve(__dirname, '..', '..', '..', 'evals', 'swarm-bench', 'bench');

  it("answers the tier's own call budget: bench_budget.CALL_BUDGET for SB7.2, forge-2.0 its own", async () => {
    const source = await fs.readFile(path.join(benchDir, 'bench_budget.py'), 'utf8');
    const stated = Number(/^CALL_BUDGET = (\d+)$/m.exec(source)?.[1]);
    expect(stated).toBeGreaterThan(0);
    expect(await readBenchCallBudget('python3', benchDir, process.env, 'sb-7.2')).toBe(stated);
    const tiers = await fs.readFile(path.join(benchDir, 'isolated_tiers.py'), 'utf8');
    const forge2 = Number(/FORGE20 = IsolatedTier\([\s\S]*?call_budget=(\d+)\)/.exec(tiers)?.[1]);
    expect(forge2).not.toBe(stated);
    expect(await readBenchCallBudget('python3', benchDir, process.env, 'forge-2.0')).toBe(forge2);
  });

  it('is null when the payload cannot answer', async () => {
    expect(await readBenchCallBudget('python3', root, process.env, 'sb-7.2')).toBeNull();
    expect(await readBenchCallBudget('python3', benchDir, process.env, 'forge-9.9')).toBeNull();
  });
});
