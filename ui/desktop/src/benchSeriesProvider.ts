import fs from 'node:fs/promises';
import path from 'node:path';
import { benchIdentityHash } from './benchSeriesComparison';

export interface BenchProviderSnapshot {
  version: 1;
  providers: string[];
  config: Record<string, unknown>;
  secrets: Record<string, unknown>;
  custom_providers: Record<string, unknown>;
}

const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value);

export function parseBenchProviderSnapshot(
  value: unknown,
  provider: string
): BenchProviderSnapshot {
  if (
    !object(value) ||
    value.version !== 1 ||
    Object.keys(value).some(
      (key) => !['version', 'providers', 'config', 'secrets', 'custom_providers'].includes(key)
    ) ||
    !Array.isArray(value.providers) ||
    value.providers.length !== 1 ||
    value.providers[0] !== provider ||
    !object(value.config) ||
    !object(value.secrets) ||
    !object(value.custom_providers) ||
    Object.keys(value.custom_providers).some(
      (name) => name !== provider || !/^[a-zA-Z0-9_-]+$/.test(name)
    )
  )
    throw new Error('The selected benchmark provider snapshot is incomplete or unsupported.');
  // Reject non-JSON fields before credentials or settings enter a native child.
  benchIdentityHash(value);
  return value as unknown as BenchProviderSnapshot;
}

export interface FrozenBenchProvider {
  root: string;
  snapshotHash: string;
  env: NodeJS.ProcessEnv;
}

/** Native entrant_config uses the ordinary config exporter against this private, fixed root. */
export async function freezeBenchProvider(
  root: string,
  snapshot: BenchProviderSnapshot,
  environment: NodeJS.ProcessEnv
): Promise<FrozenBenchProvider> {
  const config = path.join(root, 'config');
  const definitions = path.join(config, 'custom_providers');
  await fs.mkdir(definitions, { recursive: true, mode: 0o700 });
  await fs.chmod(root, 0o700);
  await fs.chmod(config, 0o700);
  await fs.chmod(definitions, 0o700);
  const privateFile = (name: string, value: unknown) =>
    fs.writeFile(name, JSON.stringify(value), { mode: 0o400, flag: 'wx' });
  await privateFile(path.join(config, 'config.yaml'), snapshot.config);
  await privateFile(path.join(config, 'secrets.yaml'), snapshot.secrets);
  for (const [name, definition] of Object.entries(snapshot.custom_providers))
    await privateFile(path.join(definitions, `${name}.json`), definition);
  const env = { ...environment };
  // Config gives environment values precedence over YAML. The original exporter already resolved
  // those values; clear them here so typed snapshot values cannot be replaced on its second read.
  for (const key of [...Object.keys(snapshot.config), ...Object.keys(snapshot.secrets)])
    delete env[key.toUpperCase()];
  env.GOOSE_PATH_ROOT = root;
  env.GOOSE_ADDITIONAL_CONFIG_FILES = '';
  env.GOOSE_DISABLE_KEYRING = '1';
  return { root, snapshotHash: benchIdentityHash(snapshot), env };
}

export function frozenBenchProviderProblem(
  frozen: FrozenBenchProvider,
  actual: BenchProviderSnapshot
): string | null {
  return benchIdentityHash(actual) === frozen.snapshotHash
    ? null
    : 'The private benchmark provider snapshot changed; the queued build was not started.';
}
