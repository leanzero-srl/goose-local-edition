import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { afterEach, describe, expect, it } from 'vitest';
import {
  freezeBenchProvider,
  frozenBenchProviderProblem,
  parseBenchProviderSnapshot,
  type BenchProviderSnapshot,
} from './benchSeriesProvider';

const snapshot = (): BenchProviderSnapshot => ({
  version: 1,
  providers: ['openrouter'],
  config: { OPENROUTER_HOST: 'https://original.invalid', GOOSE_TEMPERATURE: 0.4 },
  secrets: { OPENROUTER_API_KEY: 'inert-original-test-key' },
  custom_providers: {},
});
const directories: string[] = [];
const temporary = async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'bench-provider-test-'));
  directories.push(directory);
  return directory;
};
afterEach(async () => {
  await Promise.all(
    directories.splice(0).map((dir) => fs.rm(dir, { recursive: true, force: true }))
  );
});

const configAt = async (root: string) =>
  JSON.parse(await fs.readFile(path.join(root, 'config', 'config.yaml'), 'utf8'));

describe('fixed native provider configuration', () => {
  it('keeps both native roots on A when host Settings changes A to B and back to A', async () => {
    const host = await temporary();
    await fs.mkdir(path.join(host, 'config'));
    const settings = path.join(host, 'config', 'config.yaml');
    await fs.writeFile(settings, JSON.stringify(snapshot().config));
    const privateRoot = await temporary();
    const frozen = await freezeBenchProvider(privateRoot, snapshot(), {
      GOOSE_PATH_ROOT: host,
      OPENROUTER_HOST: 'https://environment.invalid',
      OPENROUTER_API_KEY: 'inert-other-key',
    });
    await fs.writeFile(settings, JSON.stringify({ OPENROUTER_HOST: 'https://changed.invalid' }));
    const first = await configAt(frozen.env.GOOSE_PATH_ROOT!);
    await fs.writeFile(settings, JSON.stringify(snapshot().config));
    const second = await configAt(frozen.env.GOOSE_PATH_ROOT!);
    expect(first).toEqual(snapshot().config);
    expect(second).toEqual(first);
    expect(frozen.env.OPENROUTER_HOST).toBeUndefined();
    expect(frozen.env.OPENROUTER_API_KEY).toBeUndefined();
    expect(frozen.env.GOOSE_DISABLE_KEYRING).toBe('1');
    expect(frozen.env.GOOSE_ADDITIONAL_CONFIG_FILES).toBe('');
    expect(frozenBenchProviderProblem(frozen, snapshot())).toBeNull();
    expect(
      frozenBenchProviderProblem(frozen, { ...snapshot(), config: { OPENROUTER_HOST: 'B' } })
    ).toMatch(/changed/);
  });

  it('copies custom definitions and captures the environment without retaining later mutations', async () => {
    const root = await temporary();
    const selected = parseBenchProviderSnapshot(
      {
        ...snapshot(),
        providers: ['custom_route'],
        custom_providers: {
          custom_route: {
            name: 'custom_route',
            base_url: 'https://original.invalid',
            headers: { auth: 'inert-private' },
          },
        },
      },
      'custom_route'
    );
    const environment = {
      GOOSE_PATH_ROOT: '/host',
      PATH: '/captured/bin',
      GOOSE_ADDITIONAL_CONFIG_FILES: '/host/extra.yaml',
    };
    const frozen = await freezeBenchProvider(root, selected, environment);
    environment.PATH = '/later/bin';
    expect(frozen.env.PATH).toBe('/captured/bin');
    expect(
      JSON.parse(
        await fs.readFile(
          path.join(root, 'config', 'custom_providers', 'custom_route.json'),
          'utf8'
        )
      )
    ).toEqual(selected.custom_providers.custom_route);
    expect((await fs.stat(path.join(root, 'config', 'secrets.yaml'))).mode & 0o777).toBe(0o400);
    expect((await fs.stat(root)).mode & 0o777).toBe(0o700);
    expect(JSON.stringify(frozen)).not.toContain('inert-original-test-key');
    expect(JSON.stringify(frozen)).not.toContain('inert-private');
  });

  it('does not overwrite an existing private native configuration', async () => {
    const root = await temporary();
    await freezeBenchProvider(root, snapshot(), {});
    await expect(freezeBenchProvider(root, snapshot(), {})).rejects.toThrow();
  });

  it.each([
    { ...snapshot(), providers: [] },
    { ...snapshot(), providers: ['openrouter', 'another'] },
    { ...snapshot(), config: null },
    { ...snapshot(), secrets: [] },
    { ...snapshot(), custom_providers: { '../escape': {} } },
    { ...snapshot(), unknown_future_field: {} },
  ])('refuses unsupported native snapshots instead of discarding fields', (value) => {
    expect(() => parseBenchProviderSnapshot(value, 'openrouter')).toThrow();
  });
});

const engine = process.env.BENCH_SERIES_TEST_ENGINE;
it.skipIf(!engine)(
  'unchanged native entrant_config exports the same A snapshot for both children across host A/B/A edits',
  async () => {
    const run = promisify(execFile);
    const host = await temporary();
    await fs.mkdir(path.join(host, 'config'));
    const settings = path.join(host, 'config', 'config.yaml');
    await fs.writeFile(settings, JSON.stringify(snapshot().config));
    const frozen = await freezeBenchProvider(await temporary(), snapshot(), {
      PATH: process.env.PATH,
      GOOSE_PATH_ROOT: host,
    });
    const nativeExport = async () => {
      const output = path.join(await temporary(), 'exported.json');
      const script =
        "import json, pathlib, run_build; pathlib.Path(__import__('sys').argv[1]).write_text(json.dumps(run_build.entrant_config('openrouter')))";
      await run('python3', ['-B', '-c', script, output], {
        env: {
          ...frozen.env,
          BENCH_GOOSE: engine,
          PYTHONPATH: path.resolve(process.cwd(), '../../evals/swarm-bench/bench'),
        },
      });
      return parseBenchProviderSnapshot(
        JSON.parse(await fs.readFile(output, 'utf8')),
        'openrouter'
      );
    };
    await fs.writeFile(
      settings,
      JSON.stringify({ OPENROUTER_HOST: 'https://changed.invalid', GOOSE_TEMPERATURE: 0.9 })
    );
    const first = await nativeExport();
    await fs.writeFile(settings, JSON.stringify(snapshot().config));
    const second = await nativeExport();
    expect(first).toEqual(snapshot());
    expect(second).toEqual(first);
    expect(frozenBenchProviderProblem(frozen, first)).toBeNull();
    expect(frozenBenchProviderProblem(frozen, second)).toBeNull();
    if (process.env.BENCH_SERIES_TEST_RECEIPT)
      await fs.writeFile(
        process.env.BENCH_SERIES_TEST_RECEIPT,
        JSON.stringify(
          {
            schemaVersion: 1,
            capturedAt: new Date().toISOString(),
            engine,
            actualNativeFunction: 'run_build.entrant_config',
            provider: 'openrouter',
            expectedSnapshotHash: frozen.snapshotHash,
            firstActualSnapshotHash: (await freezeBenchProvider(await temporary(), first, {}))
              .snapshotHash,
            secondActualSnapshotHash: (await freezeBenchProvider(await temporary(), second, {}))
              .snapshotHash,
            hostConfigSequence: [
              'A',
              'B before first native export',
              'A before second native export',
            ],
            keyringDisabled: frozen.env.GOOSE_DISABLE_KEYRING === '1',
            additionalConfigFilesDisabled: frozen.env.GOOSE_ADDITIONAL_CONFIG_FILES === '',
            secrets: 'inert test credentials only; no secret value retained',
            assertions: 'both complete actual native exports equal the initial snapshot',
            providerCalls: 0,
            modelCalls: 0,
          },
          null,
          2
        )
      );
  }
);
