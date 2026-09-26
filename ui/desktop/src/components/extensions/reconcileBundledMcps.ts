import type { ExtensionConfig } from '../../types/extensions';
import type { FixedExtensionEntry } from '../ConfigContext';

export type BundledMcp = Awaited<ReturnType<typeof window.electron.bundledMcps>>[number];

/**
 * What the app last wrote for each of its own servers. The path-bearing env values are stored
 * as secrets and cannot be read back, so this non-secret record is how a later start knows
 * whether they still point at its own bundle (a new browser build changes the executable path
 * while cmd and args stay the same).
 */
export const BUNDLED_MCP_RECORD_KEY = 'bundled_mcp_paths';

export interface BundledMcpWritten {
  cmd: string;
  args: string[];
  description: string;
  envs: Record<string, string>;
}
export type BundledMcpRecord = Record<string, BundledMcpWritten>;

export interface BundledMcpCorrection {
  name: string;
  enabled: boolean;
  config: ExtensionConfig;
  before: Omit<BundledMcpWritten, 'envs'> & {
    envKeys: string[];
    envs: Record<string, string> | 'not recorded (stored as secrets)';
  };
  after: BundledMcpWritten & { envKeys: string[] };
}

const written = (entry: BundledMcp): BundledMcpWritten => ({
  cmd: entry.cmd,
  args: entry.args,
  description: entry.description,
  envs: entry.envs,
});

const sameList = (a: readonly string[] = [], b: readonly string[] = []) =>
  a.length === b.length && a.every((value, index) => value === b[index]);

const sameWritten = (a: BundledMcpWritten, b: BundledMcpWritten) =>
  a.cmd === b.cmd &&
  sameList(a.args, b.args) &&
  a.description === b.description &&
  sameList(Object.keys(a.envs).sort(), Object.keys(b.envs).sort()) &&
  Object.entries(a.envs).every(([key, value]) => b.envs[key] === value);

/** The saved entry is this app's server only when its script is `…/bundled-mcps/<id>/<entry>`. */
export function isOwnBundledServer(entry: BundledMcp, saved: FixedExtensionEntry): boolean {
  if (saved.type !== 'stdio') return false;
  const script = saved.args?.[0]?.replace(/\\/g, '/');
  return Boolean(script?.endsWith(`/${entry.bundleEntry}`));
}

/** The user's env keys: everything except the ones the app writes itself. */
export function userEnvKeys(entry: BundledMcp, envKeys: readonly string[] = []): string[] {
  return envKeys.filter((key) => !entry.managedEnvKeys.includes(key));
}

/**
 * The entries whose command, script, description or app-written env values no longer match
 * this app's own bundle — e.g. a development run wrote its source tree's Electron and script
 * into config.yaml and the installed app kept launching them. Only a packaged app corrects:
 * a development run pointing the user's real config at a source tree is how the drift began.
 * Unconfigured servers and same-named servers that do not run this app's script are left alone.
 */
export function planBundledMcpCorrections(
  entries: readonly BundledMcp[],
  extensions: readonly FixedExtensionEntry[],
  record: BundledMcpRecord
): BundledMcpCorrection[] {
  const corrections: BundledMcpCorrection[] = [];
  for (const entry of entries) {
    if (!entry.packaged) continue;
    const saved = extensions.find((extension) => extension.name === entry.name);
    if (!saved || saved.type !== 'stdio' || !isOwnBundledServer(entry, saved)) continue;
    const want = written(entry);
    const savedEnvKeys = saved.env_keys ?? [];
    const savedManagedKeys = savedEnvKeys.filter((key) => entry.managedEnvKeys.includes(key));
    const previous = record[entry.name];
    const current =
      saved.cmd === want.cmd &&
      sameList(saved.args, want.args) &&
      saved.description === want.description &&
      sameList(savedManagedKeys.slice().sort(), Object.keys(want.envs).sort()) &&
      previous !== undefined &&
      sameWritten(previous, want);
    if (current) continue;
    const envKeys = userEnvKeys(entry, savedEnvKeys);
    const { enabled, configKey: _configKey, ...config } = saved;
    corrections.push({
      name: entry.name,
      enabled,
      config: {
        ...config,
        type: 'stdio',
        name: entry.name,
        description: want.description,
        cmd: want.cmd,
        args: want.args,
        envs: want.envs,
        env_keys: envKeys,
        timeout: saved.timeout ?? entry.timeout,
      },
      before: {
        cmd: saved.cmd,
        args: saved.args ?? [],
        description: saved.description ?? '',
        envKeys: savedEnvKeys,
        envs: previous?.envs ?? 'not recorded (stored as secrets)',
      },
      after: { ...want, envKeys: [...envKeys, ...Object.keys(want.envs)] },
    });
  }
  return corrections;
}

/**
 * Point every configured bundled server at this app's own bundle, keeping the user's enabled
 * flag, env keys (their Serper key, folders) and timeout. Each correction is logged once, with
 * before and after; a start with nothing to correct writes and logs nothing.
 */
export async function reconcileBundledMcps(deps: {
  entries: readonly BundledMcp[];
  extensions: readonly FixedExtensionEntry[];
  record: BundledMcpRecord;
  add: (config: ExtensionConfig, enabled: boolean) => Promise<void>;
  saveRecord: (record: BundledMcpRecord) => Promise<void>;
  log: (line: string) => void;
}): Promise<BundledMcpCorrection[]> {
  const corrections = planBundledMcpCorrections(deps.entries, deps.extensions, deps.record);
  if (corrections.length === 0) return corrections;
  const record = { ...deps.record };
  for (const correction of corrections) {
    await deps.add(correction.config, correction.enabled);
    const entry = deps.entries.find((item) => item.name === correction.name);
    if (entry) record[correction.name] = written(entry);
    deps.log(
      `[bundled-mcps] corrected "${correction.name}" to this app's bundle: ${JSON.stringify({
        before: correction.before,
        after: correction.after,
      })}`
    );
  }
  await deps.saveRecord(record);
  return corrections;
}
