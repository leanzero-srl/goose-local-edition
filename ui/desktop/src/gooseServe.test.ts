import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  buildGooseServeEnv,
  buildLocalServeUrls,
  findGooseBinaryPath,
  GOOSED_SIGKILL_AFTER_MS,
  startGooseServe,
  withSystemSbin,
} from './gooseServe';

const binaryName = process.platform === 'win32' ? 'goose.exe' : 'goose';
const tempDirs: string[] = [];
const originalCwd = process.cwd();
type ReadinessFetchInit = Parameters<typeof globalThis.fetch>[1];

function makeTempDir(): string {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'goose-serve-test-'));
  tempDirs.push(tempDir);
  return tempDir;
}

function makeFile(filePath: string): string {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, '');
  fs.chmodSync(filePath, 0o755);
  return filePath;
}

function makeExecutable(filePath: string, contents: string): string {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, contents);
  fs.chmodSync(filePath, 0o755);
  return filePath;
}

async function waitForFileLines(filePath: string): Promise<string[]> {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    if (fs.existsSync(filePath)) {
      return fs.readFileSync(filePath, 'utf8').trim().split('\n');
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`Timed out waiting for ${filePath}`);
}

describe('findGooseBinaryPath', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    process.chdir(originalCwd);

    while (tempDirs.length > 0) {
      const tempDir = tempDirs.pop();
      if (tempDir) {
        fs.rmSync(tempDir, { recursive: true, force: true });
      }
    }
  });

  it('uses GOOSE_BINARY in development builds', () => {
    const tempDir = makeTempDir();
    const overridePath = makeFile(path.join(tempDir, 'override-goose'));
    vi.stubEnv('GOOSE_BINARY', overridePath);

    expect(findGooseBinaryPath({ isPackaged: false })).toBe(overridePath);
  });

  it('rejects GOOSE_BINARY in packaged builds', () => {
    const tempDir = makeTempDir();
    const resourcesPath = path.join(tempDir, 'resources');
    const overridePath = makeFile(path.join(tempDir, 'override-goose'));
    makeFile(path.join(resourcesPath, 'bin', binaryName));
    vi.stubEnv('GOOSE_BINARY', overridePath);

    expect(() => findGooseBinaryPath({ isPackaged: true, resourcesPath })).toThrow(
      'GOOSE_BINARY is only supported in development builds'
    );
  });

  it('prefers the staged binary over target builds in development builds', () => {
    const tempDir = makeTempDir();
    const desktopDir = path.join(tempDir, 'ui', 'desktop');
    const stagedPath = makeFile(path.join(desktopDir, 'src', 'bin', binaryName));
    const debugPath = makeFile(path.join(tempDir, 'target', 'debug', binaryName));
    const releasePath = makeFile(path.join(tempDir, 'target', 'release', binaryName));
    process.chdir(desktopDir);

    const resolvedPath = findGooseBinaryPath({ isPackaged: false });
    expect(fs.realpathSync(resolvedPath)).toBe(fs.realpathSync(stagedPath));
    expect(fs.realpathSync(resolvedPath)).not.toBe(fs.realpathSync(releasePath));
    expect(fs.realpathSync(resolvedPath)).not.toBe(fs.realpathSync(debugPath));
  });

  it('uses the bundled goose binary in packaged builds', () => {
    const tempDir = makeTempDir();
    const resourcesPath = path.join(tempDir, 'resources');
    const bundledPath = makeFile(path.join(resourcesPath, 'bin', binaryName));

    expect(findGooseBinaryPath({ isPackaged: true, resourcesPath })).toBe(bundledPath);
  });
});

describe('buildLocalServeUrls', () => {
  it('builds HTTP and WS URLs', () => {
    expect(buildLocalServeUrls(1234, 'secret', 'http')).toEqual({
      httpBaseUrl: 'http://127.0.0.1:1234',
      statusUrl: 'http://127.0.0.1:1234/status',
      healthUrl: 'http://127.0.0.1:1234/health',
      acpUrl: 'ws://127.0.0.1:1234/acp?token=secret',
      redactedAcpUrl: 'ws://127.0.0.1:1234/acp?token=REDACTED',
    });
  });

  it('builds HTTPS and WSS URLs', () => {
    expect(buildLocalServeUrls(1234, 'secret', 'https')).toEqual({
      httpBaseUrl: 'https://127.0.0.1:1234',
      statusUrl: 'https://127.0.0.1:1234/status',
      healthUrl: 'https://127.0.0.1:1234/health',
      acpUrl: 'wss://127.0.0.1:1234/acp?token=secret',
      redactedAcpUrl: 'wss://127.0.0.1:1234/acp?token=REDACTED',
    });
  });
});

describe('startGooseServe', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    process.chdir(originalCwd);

    while (tempDirs.length > 0) {
      const tempDir = tempDirs.pop();
      if (tempDir) {
        fs.rmSync(tempDir, { recursive: true, force: true });
      }
    }
  });

  it.skipIf(process.platform === 'win32')('uses the injected readiness fetch', async () => {
    const tempDir = makeTempDir();
    const goosePath = makeExecutable(
      path.join(tempDir, 'goose'),
      '#!/usr/bin/env sh\nwhile true; do sleep 1; done\n'
    );
    vi.stubEnv('GOOSE_BINARY', goosePath);

    const readinessUrls: string[] = [];
    const readinessFetch = vi.fn(async (input: string, _init?: ReadinessFetchInit) => {
      readinessUrls.push(input);
      return new Response(null, { status: 200 });
    });

    const result = await startGooseServe({
      serverSecret: 'test-secret',
      dir: tempDir,
      readinessFetch,
    });

    try {
      expect(readinessFetch).toHaveBeenCalledTimes(1);
      expect(readinessUrls[0]).toMatch(/^http:\/\/127\.0\.0\.1:\d+\/status$/);
    } finally {
      await result.cleanup();
    }
  });

  it.skipIf(process.platform === 'win32')('captures the TLS fingerprint from stdout', async () => {
    const tempDir = makeTempDir();
    const goosePath = makeExecutable(
      path.join(tempDir, 'goose'),
      [
        '#!/usr/bin/env sh',
        'printf "GOOSED_CERT_FINGERPRINT=AA:BB:CC\\n"',
        'while true; do sleep 1; done',
        '',
      ].join('\n')
    );
    vi.stubEnv('GOOSE_BINARY', goosePath);

    let fingerprintLogged!: () => void;
    const fingerprintSeen = new Promise<void>((resolve) => {
      fingerprintLogged = resolve;
    });
    const logger = {
      info: vi.fn((message: unknown) => {
        if (String(message).includes('Pinned cert fingerprint')) {
          fingerprintLogged();
        }
      }),
      error: vi.fn(),
    };
    const readinessFetch = vi.fn(async () => {
      await fingerprintSeen;
      return new Response(null, { status: 200 });
    });

    const result = await startGooseServe({
      serverSecret: 'test-secret',
      dir: tempDir,
      logger,
      readinessFetch,
    });

    try {
      expect(result.certFingerprint).toBe('AA:BB:CC');
    } finally {
      await result.cleanup();
    }
  });

  it.skipIf(process.platform === 'win32')('uses TLS URLs and args when TLS is enabled', async () => {
    const tempDir = makeTempDir();
    const argsPath = path.join(tempDir, 'args.txt');
    const goosePath = makeExecutable(
      path.join(tempDir, 'goose'),
      [
        '#!/usr/bin/env sh',
        'printf "%s\\n" "$@" > "$TEST_ARGS_PATH"',
        'printf "GOOSED_CERT_FINGERPRINT=DD:EE:FF\\n"',
        'while true; do sleep 1; done',
        '',
      ].join('\n')
    );
    vi.stubEnv('GOOSE_BINARY', goosePath);

    const readinessUrls: string[] = [];
    const logger = {
      info: vi.fn(),
      error: vi.fn(),
    };
    const readinessFetch = vi.fn(async (input: string, _init?: ReadinessFetchInit) => {
      readinessUrls.push(input);
      return new Response(null, { status: 200 });
    });

    const result = await startGooseServe({
      serverSecret: 'test-secret',
      dir: tempDir,
      tls: true,
      env: {
        TEST_ARGS_PATH: argsPath,
      },
      logger,
      readinessFetch,
    });

    try {
      expect(readinessUrls[0]).toMatch(/^https:\/\/127\.0\.0\.1:\d+\/status$/);
      expect(result.acpUrl).toMatch(/^wss:\/\/127\.0\.0\.1:\d+\/acp\?token=test-secret$/);
      expect(result.certFingerprint).toBe('DD:EE:FF');
      await expect(waitForFileLines(argsPath)).resolves.toContain('--tls');
    } finally {
      await result.cleanup();
    }
  });

  it.skipIf(process.platform === 'win32')('waits for TLS fingerprint after readiness succeeds', async () => {
    const tempDir = makeTempDir();
    const goosePath = makeExecutable(
      path.join(tempDir, 'goose'),
      [
        '#!/usr/bin/env sh',
        'sleep 0.1',
        'printf "GOOSED_CERT_FINGERPRINT=11:22:33\\n"',
        'while true; do sleep 1; done',
        '',
      ].join('\n')
    );
    vi.stubEnv('GOOSE_BINARY', goosePath);

    const readinessFetch = vi.fn(async () => new Response(null, { status: 200 }));

    const result = await startGooseServe({
      serverSecret: 'test-secret',
      dir: tempDir,
      tls: true,
      readinessFetch,
    });

    try {
      expect(readinessFetch).toHaveBeenCalled();
      expect(result.certFingerprint).toBe('11:22:33');
    } finally {
      await result.cleanup();
    }
  });
});

describe('buildGooseServeEnv — bundled tailscaled wiring', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  const tailscaledName = process.platform === 'win32' ? 'tailscaled.exe' : 'tailscaled';
  const tailscaleName = process.platform === 'win32' ? 'tailscale.exe' : 'tailscale';

  function binDirWith(files: string[]): { dir: string; goose: string } {
    const dir = makeTempDir();
    for (const f of files) {
      fs.writeFileSync(path.join(dir, f), 'x');
    }
    return { dir, goose: path.join(dir, binaryName) };
  }

  it('points LEANZERO_TAILSCALED/CLI at the binaries bundled next to goosed', () => {
    vi.stubEnv('LEANZERO_TAILSCALED', '');
    vi.stubEnv('LEANZERO_TAILSCALE_CLI', '');
    const { dir, goose } = binDirWith([binaryName, tailscaledName, tailscaleName]);
    const env = buildGooseServeEnv('secret', goose, {});
    expect(env.LEANZERO_TAILSCALED).toBe(path.join(dir, tailscaledName));
    expect(env.LEANZERO_TAILSCALE_CLI).toBe(path.join(dir, tailscaleName));
  });

  it('leaves the vars unset when no binary is bundled (discovery falls through to PATH)', () => {
    vi.stubEnv('LEANZERO_TAILSCALED', '');
    vi.stubEnv('LEANZERO_TAILSCALE_CLI', '');
    const { goose } = binDirWith([binaryName]);
    const env = buildGooseServeEnv('secret', goose, {});
    expect(env.LEANZERO_TAILSCALED).toBeFalsy();
    expect(env.LEANZERO_TAILSCALE_CLI).toBeFalsy();
  });

  it('lets an explicit override win over the bundled binary', () => {
    vi.stubEnv('LEANZERO_TAILSCALED', '');
    const { goose } = binDirWith([binaryName, tailscaledName, tailscaleName]);
    const env = buildGooseServeEnv('secret', goose, { LEANZERO_TAILSCALED: '/custom/tailscaled' });
    expect(env.LEANZERO_TAILSCALED).toBe('/custom/tailscaled');
  });
});

describe('stop — the SIGKILL fallback covers goosed\'s own teardown', () => {
  // goosed's teardown on SIGTERM is bounded only by its supervisors' per-pid grace windows:
  // the mesh daemon (one 50 × 100 ms leg), the engine sidecar (two: terminate + release_port)
  // and the status probe that gates the unmount (reqwest 5 s). Cutting SIGKILL in before
  // that ceiling re-creates the orphans this constant exists to prevent.
  it('waits at least the mesh + engine + probe ceilings before SIGKILL', () => {
    const perPidGraceMs = 50 * 100;
    const meshCeiling = perPidGraceMs;
    const engineCeiling = 2 * perPidGraceMs;
    const probeCeiling = 5000;
    expect(GOOSED_SIGKILL_AFTER_MS).toBeGreaterThanOrEqual(meshCeiling + engineCeiling + probeCeiling);
  });
});

describe('withSystemSbin — goosed can reach lsof from its PATH', () => {
  it('appends /usr/sbin and /sbin when the PATH lacks them, keeping the existing order', () => {
    expect(withSystemSbin('/app/bin:/usr/bin:/bin', 'darwin')).toBe('/app/bin:/usr/bin:/bin:/usr/sbin:/sbin');
  });

  it('adds only the one that is missing', () => {
    expect(withSystemSbin('/app/bin:/usr/sbin:/usr/bin', 'darwin')).toBe('/app/bin:/usr/sbin:/usr/bin:/sbin');
  });

  it('leaves a PATH that already carries both untouched', () => {
    const value = '/app/bin:/usr/sbin:/sbin:/usr/bin:/bin';
    expect(withSystemSbin(value, 'linux')).toBe(value);
  });

  it('does nothing on Windows', () => {
    expect(withSystemSbin('C:\\app\\bin;C:\\Windows', 'win32')).toBe('C:\\app\\bin;C:\\Windows');
  });

  it.skipIf(process.platform === 'win32')('buildGooseServeEnv hands goosed a PATH with /usr/sbin', () => {
    vi.stubEnv('PATH', '/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin');
    const dir = makeTempDir();
    const goose = path.join(dir, binaryName);
    fs.writeFileSync(goose, 'x');
    const env = buildGooseServeEnv('secret', goose, {});
    const entries = (env.PATH ?? '').split(path.delimiter);
    expect(entries[0]).toBe(dir);
    expect(entries).toContain('/usr/sbin');
    expect(entries).toContain('/sbin');
  });
});
