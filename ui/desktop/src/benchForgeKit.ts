import path from 'node:path';
import { execFile } from 'node:child_process';
import type { ForgeKitStatus } from './benchForgeKitTypes';

export type { ForgeKitStatus } from './benchForgeKitTypes';

/**
 * Reads the kit's readiness and the Forge tier's published run policy from the payload's own Python —
 * one rule for "ready" (forge_kit.status) and one source for the numbers the view shows (the call budget,
 * the pinned effort), so the app never restates a policy that lives in bench/. There is no default spend
 * limit (owner 2026-10-02: an OpenRouter run runs until the credits go; the field in the form is the only stop).
 */
export const FORGE_KIT_STATUS_SCRIPT = [
  'import json, forge_kit, bench_budget, isolated_tiers',
  "print(json.dumps({**forge_kit.status(), 'call_budget': bench_budget.CALL_BUDGET,",
  "                  'reasoning_effort': isolated_tiers.FORGE10.reasoning_effort}))",
].join('\n');

export interface ForgeKitRuntime {
  python: string;
  node: string;
  env: Record<string, string>;
}

/** Where the kit is materialised: beside the Benchmark tools, under the app's own benchmark root. */
export const forgeKitCache = (benchWorkRoot: string) => path.join(benchWorkRoot, 'forge-kit');

/** The environment both the status read and `ensure` run with: npm is the runtime Node's sibling. */
export function forgeKitEnv(runtime: ForgeKitRuntime, cache: string): NodeJS.ProcessEnv {
  return {
    ...process.env,
    ...runtime.env,
    GOOSE_SWARM_RENDER_NODE: runtime.node,
    FORGE_KIT_CACHE: cache,
    PYTHONDONTWRITEBYTECODE: '1',
  };
}

const lastLines = (text: string) => text.trim().split('\n').slice(-6).join('\n');

function run(
  python: string,
  args: string[],
  cwd: string,
  env: NodeJS.ProcessEnv
): Promise<{ stdout: string; stderr: string }> {
  return new Promise((resolve, reject) => {
    execFile(python, args, { cwd, env, maxBuffer: 8 * 1024 * 1024 }, (error, stdout, stderr) => {
      if (error) reject(new Error(lastLines(String(stderr)) || error.message));
      else resolve({ stdout: String(stdout), stderr: String(stderr) });
    });
  });
}

/** Parses the status script's JSON line into what the view shows. A malformed answer is an error, never "ready". */
export function parseForgeKitStatus(stdout: string): ForgeKitStatus {
  const line = stdout.trim().split('\n').pop() ?? '';
  let raw: Record<string, unknown>;
  try {
    raw = JSON.parse(line) as Record<string, unknown>;
  } catch {
    return {
      state: 'error',
      error: `The Forge kit status could not be read: ${line.slice(0, 200)}`,
    };
  }
  if (typeof raw.ready !== 'boolean' || !Array.isArray(raw.missing))
    return { state: 'error', error: 'The Forge kit status carried no readiness.' };
  return {
    state: raw.ready ? 'ready' : 'missing',
    missing: raw.missing.map(String),
    ...(typeof raw.kit_lock_sha256 === 'string' ? { kitLockSha256: raw.kit_lock_sha256 } : {}),
    ...(typeof raw.call_budget === 'number' ? { callBudget: raw.call_budget } : {}),
    reasoningEffort: typeof raw.reasoning_effort === 'string' ? raw.reasoning_effort : null,
  };
}

export async function readForgeKitStatus(
  payloadDir: string,
  runtime: ForgeKitRuntime,
  cache: string
): Promise<ForgeKitStatus> {
  try {
    const { stdout } = await run(
      runtime.python,
      ['-B', '-c', FORGE_KIT_STATUS_SCRIPT],
      path.join(payloadDir, 'bench'),
      forgeKitEnv(runtime, cache)
    );
    return parseForgeKitStatus(stdout);
  } catch (error) {
    return { state: 'error', error: error instanceof Error ? error.message : String(error) };
  }
}

/** `forge_kit.py ensure`: npm ci from the committed lockfiles and the runtime wrapper by sha256. Throws its REFUSED text. */
export async function prepareForgeKit(
  payloadDir: string,
  runtime: ForgeKitRuntime,
  cache: string
): Promise<void> {
  await run(
    runtime.python,
    ['-B', '-u', path.join(payloadDir, 'bench', 'forge_kit.py'), 'ensure'],
    path.join(payloadDir, 'bench'),
    forgeKitEnv(runtime, cache)
  );
}
