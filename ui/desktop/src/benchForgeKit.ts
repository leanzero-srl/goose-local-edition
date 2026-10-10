import path from 'node:path';
import { execFile } from 'node:child_process';
import type { ForgeKitStatus } from './benchForgeKitTypes';
import { FORGE_BENCHMARK_TIER } from './benchTierPayload';
import { TIER_SCORER } from './components/benchmark/baselines';

export type { ForgeKitStatus } from './benchForgeKitTypes';

/** The bundled Forge era's kit module in the payload's bench/: forge2_kit.py materialises forge2/kit
 *  (forge_kit.py is forge-1.0's, whose runs this app no longer launches or re-scores). */
export const FORGE_KIT_MODULE = 'forge2_kit';

/**
 * Reads the bundled era's kit readiness and its tier's published run policy from the payload's own Python —
 * one rule for "ready" (the kit module's status()) and one source for the numbers the view shows (the
 * tier's call budget and pinned effort in isolated_tiers), so the app never restates a policy that lives in
 * bench/. There is no default spend limit (owner 2026-10-02: an OpenRouter run runs until the credits go;
 * the field in the form is the only stop).
 */
export const FORGE_KIT_STATUS_SCRIPT = [
  `import json, isolated_tiers, ${FORGE_KIT_MODULE}`,
  `tier = isolated_tiers.BY_VERSION['${TIER_SCORER[FORGE_BENCHMARK_TIER]}']`,
  `print(json.dumps({**${FORGE_KIT_MODULE}.status(), 'call_budget': tier.call_budget,`,
  "                  'reasoning_effort': tier.reasoning_effort}))",
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

/** The kit module's `ensure`: npm ci from the committed lockfiles and the runtime wrapper by sha256. Throws its REFUSED text. */
export async function prepareForgeKit(
  payloadDir: string,
  runtime: ForgeKitRuntime,
  cache: string
): Promise<void> {
  await run(
    runtime.python,
    ['-B', '-u', path.join(payloadDir, 'bench', `${FORGE_KIT_MODULE}.py`), 'ensure'],
    path.join(payloadDir, 'bench'),
    forgeKitEnv(runtime, cache)
  );
}
