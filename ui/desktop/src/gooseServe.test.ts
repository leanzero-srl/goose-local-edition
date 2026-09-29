import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { pollUntil, testClock } from './test/testClock';
import {
  buildGooseServeEnv,
  buildLocalServeUrls,
  findGooseBinaryPath,
  GOOSED_FOLLOWS_PARENT_ARG,
  GOOSED_SIGKILL_AFTER_MS,
  type GooseServeResult,
  startGooseServe,
  type StartGooseServeOptions,
  withSystemSbin,
} from './gooseServe';

const binaryName = process.platform === 'win32' ? 'goose.exe' : 'goose';
const tempDirs: string[] = [];
const originalCwd = process.cwd();
type ReadinessFetchInit = Parameters<typeof globalThis.fetch>[1];

// Q-456: the fake records its argv in a sibling file and renames it into place, so the file exists
// only once it is whole. `printf … > "$TEST_ARGS_PATH"` creates the file BEFORE printf writes, and a
// test that polls for the file's existence read it empty — CI c5a22884d saw argv [''].
const RECORD_ARGV =
  'printf "%s\\n" "$@" > "$TEST_ARGS_PATH.part" && mv "$TEST_ARGS_PATH.part" "$TEST_ARGS_PATH"';

// Q-140/Q-456: these tests exec a freshly written fake goosed, and macOS holds the first exec of a new
// script for an assessment that serializes machine-wide — under load two TLS tests ran past vitest's
// 5 s default (5,023 ms and 5,005 ms). This is a hang guard, not a speed limit: every wait inside
// runs on the test clock (testClock.ts), so a slow machine is never a failure while the output is right.
const REAL_GOOSED_HANG_MS = 60_000;

// Every goosed a test starts is stopped, and its exit awaited, before the next test begins — also when
// the test failed or timed out before its own stop, so no fake outlives the test that spawned it.
const startedServes: GooseServeResult[] = [];

async function startServe(options: StartGooseServeOptions): Promise<GooseServeResult> {
  const result = await startGooseServe(options);
  startedServes.push(result);
  return result;
}

async function stopStartedServes(): Promise<void> {
  await Promise.all(startedServes.splice(0).map((result) => result.cleanup()));
}

function removeTempDirs(): void {
  while (tempDirs.length > 0) {
    const tempDir = tempDirs.pop();
    if (tempDir) fs.rmSync(tempDir, { recursive: true, force: true });
  }
}

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

async function waitForRecordedArgv(argsPath: string): Promise<string[]> {
  await pollUntil(() => fs.existsSync(argsPath), 'the fake goosed to record its argv', 10);
  return fs.readFileSync(argsPath, 'utf8').trim().split('\n');
}

describe('findGooseBinaryPath', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    process.chdir(originalCwd);
    removeTempDirs();
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

describe('startGooseServe', { timeout: REAL_GOOSED_HANG_MS }, () => {
  afterEach(async () => {
    await stopStartedServes();
    vi.unstubAllEnvs();
    process.chdir(originalCwd);
    removeTempDirs();
  }, REAL_GOOSED_HANG_MS);

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

    await startServe({
      serverSecret: 'test-secret',
      dir: tempDir,
      stderrLogPath: path.join(tempDir, 'stderr', 'goose-serve-stderr.log'),
      readinessFetch,
    });

    expect(readinessFetch).toHaveBeenCalledTimes(1);
    expect(readinessUrls[0]).toMatch(/^http:\/\/127\.0\.0\.1:\d+\/status$/);
  });

  // Q-257: one goosed serves every window, so its stderr goes to the app-wide file the caller names —
  // never into the first window's `.swarm/`.
  it.skipIf(process.platform === 'win32')(
    "appends goosed's stderr to the named file and writes nothing under the dir",
    async () => {
      const tempDir = makeTempDir();
      const goosePath = makeExecutable(
        path.join(tempDir, 'goose'),
        '#!/usr/bin/env sh\necho "goosed-said-this" >&2\nwhile true; do sleep 1; done\n'
      );
      vi.stubEnv('GOOSE_BINARY', goosePath);
      const stderrLogPath = path.join(tempDir, 'logs', 'goose-serve-stderr.log');

      const result = await startServe({
        serverSecret: 'test-secret',
        dir: tempDir,
        stderrLogPath,
        // Ready once the fake goosed has spoken, so the stop cannot outrun its one line.
        readinessFetch: async () => {
          await vi.waitFor(
            () => expect(fs.readFileSync(stderrLogPath, 'utf8')).toContain('goosed-said-this'),
            { timeout: testClock() }
          );
          return new Response(null, { status: 200 });
        },
      });
      await result.cleanup();

      // The sink flushes after the exit event; the test waits for the bytes, the product never does.
      await vi.waitFor(
        () => expect(fs.readFileSync(stderrLogPath, 'utf8')).toMatch(/===== goose serve exited /),
        { timeout: testClock() }
      );
      expect(fs.existsSync(path.join(tempDir, '.swarm'))).toBe(false);
    }
  );

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

    const result = await startServe({
      serverSecret: 'test-secret',
      dir: tempDir,
      stderrLogPath: path.join(tempDir, 'stderr', 'goose-serve-stderr.log'),
      logger,
      readinessFetch,
    });

    expect(result.certFingerprint).toBe('AA:BB:CC');
  });

  it.skipIf(process.platform === 'win32')(
    'uses TLS URLs and args when TLS is enabled',
    async () => {
      const tempDir = makeTempDir();
      const argsPath = path.join(tempDir, 'args.txt');
      const goosePath = makeExecutable(
        path.join(tempDir, 'goose'),
        [
          '#!/usr/bin/env sh',
          RECORD_ARGV,
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

      const result = await startServe({
        serverSecret: 'test-secret',
        dir: tempDir,
        stderrLogPath: path.join(tempDir, 'stderr', 'goose-serve-stderr.log'),
        tls: true,
        env: {
          TEST_ARGS_PATH: argsPath,
        },
        logger,
        readinessFetch,
      });

      expect(readinessUrls[0]).toMatch(/^https:\/\/127\.0\.0\.1:\d+\/status$/);
      expect(result.acpUrl).toMatch(/^wss:\/\/127\.0\.0\.1:\d+\/acp\?token=test-secret$/);
      expect(result.certFingerprint).toBe('DD:EE:FF');
      expect(await waitForRecordedArgv(argsPath)).toContain('--tls');
    }
  );

  it.skipIf(process.platform === 'win32')(
    'waits for TLS fingerprint after readiness succeeds',
    async () => {
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

      const result = await startServe({
        serverSecret: 'test-secret',
        dir: tempDir,
        stderrLogPath: path.join(tempDir, 'stderr', 'goose-serve-stderr.log'),
        tls: true,
        readinessFetch,
      });

      expect(readinessFetch).toHaveBeenCalled();
      expect(result.certFingerprint).toBe('11:22:33');
    }
  );
});

// Q-223: the app's quit ended its goosed neither reliably nor with a wait. These drive startGooseServe
// against fake goosed scripts with the real signal and pipe plumbing.
describe(
  'startGooseServe — goosed ends with its app, and the stop waits for its exit (Q-223)',
  { timeout: REAL_GOOSED_HANG_MS },
  () => {
    afterEach(async () => {
      await stopStartedServes();
      vi.unstubAllEnvs();
      removeTempDirs();
    }, REAL_GOOSED_HANG_MS);

    const ready = () => vi.fn(async () => new Response(null, { status: 200 }));
    const quiet = () => ({ info: vi.fn(), error: vi.fn() });
    // Ready only once the fake has installed its TERM trap (it touches `trapped`), as a real goosed
    // installs its signal handlers before it binds — a SIGTERM earlier would kill the shell outright.
    const readyOnceTrapped = (dir: string) =>
      vi.fn(
        async () =>
          new Response(null, { status: fs.existsSync(path.join(dir, 'trapped')) ? 200 : 503 })
      );

    const waitFor = (check: () => boolean, what: string) => pollUntil(check, what, 25);

    it.skipIf(process.platform === 'win32')(
      'hands goosed a stdin pipe held open for its life, and asks it to exit on EOF',
      async () => {
        const tempDir = makeTempDir();
        const argsPath = path.join(tempDir, 'args.txt');
        const eofPath = path.join(tempDir, 'eof');
        makeExecutable(
          path.join(tempDir, 'goose'),
          [
            '#!/usr/bin/env sh',
            RECORD_ARGV,
            'cat > /dev/null',
            'echo eof > "$TEST_EOF_PATH"',
            '',
          ].join('\n')
        );
        vi.stubEnv('GOOSE_BINARY', path.join(tempDir, 'goose'));

        const result = await startServe({
          serverSecret: 'test-secret',
          dir: tempDir,
          stderrLogPath: path.join(tempDir, 'stderr', 'goose-serve-stderr.log'),
          env: { TEST_ARGS_PATH: argsPath, TEST_EOF_PATH: eofPath },
          logger: quiet(),
          readinessFetch: ready(),
        });
        // The fake records its argv and then blocks in `cat`: once the file exists it is reading.
        expect(await waitForRecordedArgv(argsPath)).toContain(GOOSED_FOLLOWS_PARENT_ARG);
        await new Promise((resolve) => setTimeout(resolve, 300));
        expect(fs.existsSync(eofPath)).toBe(false);
        expect(result.hasExited()).toBe(false);

        // What the kernel does to the pipe when this process dies, however it dies.
        result.process.stdin?.destroy();
        await waitFor(() => result.hasExited(), 'the fake goosed to exit on EOF');
        expect(fs.readFileSync(eofPath, 'utf8').trim()).toBe('eof');
      }
    );

    it.skipIf(process.platform === 'win32')(
      'resolves the stop only once goosed has EXITED, for every caller',
      async () => {
        const tempDir = makeTempDir();
        const donePath = path.join(tempDir, 'teardown-done');
        makeExecutable(
          path.join(tempDir, 'goose'),
          [
            '#!/usr/bin/env sh',
            // goosed's own teardown: it takes a while, then exits on its own.
            'trap \'sleep 0.4; echo done > "$TEST_DONE_PATH"; exit 143\' TERM',
            'touch "$TEST_DIR/trapped"',
            'while true; do sleep 0.05; done',
            '',
          ].join('\n')
        );
        vi.stubEnv('GOOSE_BINARY', path.join(tempDir, 'goose'));

        const result = await startServe({
          serverSecret: 'test-secret',
          dir: tempDir,
          stderrLogPath: path.join(tempDir, 'stderr', 'goose-serve-stderr.log'),
          env: { TEST_DONE_PATH: donePath, TEST_DIR: tempDir },
          logger: quiet(),
          readinessFetch: readyOnceTrapped(tempDir),
        });
        const release = result.cleanup();
        const quit = result.cleanup();
        await Promise.all([release, quit]);
        expect(fs.existsSync(donePath)).toBe(true);
        expect(result.hasExited()).toBe(true);
        expect(result.getExitDetails().code).toBe(143);
      }
    );

    it.skipIf(process.platform === 'win32')(
      'SIGKILLs a goosed that ignores SIGTERM only after the grace, and waits for that exit',
      async () => {
        const tempDir = makeTempDir();
        makeExecutable(
          path.join(tempDir, 'goose'),
          [
            '#!/usr/bin/env sh',
            "trap '' TERM",
            'touch "$TEST_DIR/trapped"',
            'while true; do sleep 0.05; done',
            '',
          ].join('\n')
        );
        vi.stubEnv('GOOSE_BINARY', path.join(tempDir, 'goose'));
        const logger = { info: vi.fn(), error: vi.fn() };

        const result = await startServe({
          serverSecret: 'test-secret',
          dir: tempDir,
          stderrLogPath: path.join(tempDir, 'stderr', 'goose-serve-stderr.log'),
          env: { TEST_DIR: tempDir },
          logger,
          readinessFetch: readyOnceTrapped(tempDir),
          sigkillAfterMs: 300,
        });
        const started = Date.now();
        await result.cleanup();
        expect(Date.now() - started).toBeGreaterThanOrEqual(300);
        expect(result.hasExited()).toBe(true);
        expect(result.getExitDetails().signal).toBe('SIGKILL');
        expect(logger.error).toHaveBeenCalledWith(expect.stringContaining('sending SIGKILL'));
      }
    );
  }
);

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

describe('buildGooseServeEnv — the tool shim directory (Q-102)', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('keeps the shims first on PATH for MCP launches and names their directory for the shell tool', () => {
    vi.stubEnv('GOOSE_TOOL_SHIM_DIR', '');
    const dir = makeTempDir();
    const goose = path.join(dir, binaryName);
    fs.writeFileSync(goose, 'x');
    fs.writeFileSync(path.join(dir, 'goose-shim-common.sh'), 'x');
    const env = buildGooseServeEnv('secret', goose, {});
    expect((env.PATH ?? '').split(path.delimiter)[0]).toBe(dir);
    expect(env.GOOSE_TOOL_SHIM_DIR).toBe(dir);
  });

  it('names nothing when the binary sits somewhere without the shims (a cargo target dir)', () => {
    vi.stubEnv('GOOSE_TOOL_SHIM_DIR', '');
    const dir = makeTempDir();
    const goose = path.join(dir, binaryName);
    fs.writeFileSync(goose, 'x');
    const env = buildGooseServeEnv('secret', goose, {});
    expect(env.GOOSE_TOOL_SHIM_DIR).toBeFalsy();
  });
});

describe("stop — the SIGKILL fallback covers goosed's own teardown", () => {
  // goosed's teardown on SIGTERM is bounded only by its supervisors' own windows: the stdio
  // extension children (one shared 50 × 100 ms window — Q-138), the going-away notice (the Link's
  // 5 s connect timeout), the engine sidecar (two legs: terminate + release_port) and the status
  // probe that gates the unmount (reqwest 5 s), the split's local rank (three legs) and its peer
  // rank over the Link (connect 5 s + three legs on the peer — Q-242: it runs before the mesh
  // stops now, so it really waits on the peer), and the mesh daemon (one leg). Cutting SIGKILL in
  // before that ceiling re-creates the orphans this constant exists to prevent.
  it('waits at least every teardown step’s ceiling before SIGKILL', () => {
    const perPidGraceMs = 50 * 100;
    const linkConnectMs = 5000;
    const stdioCeiling = perPidGraceMs;
    const noticeCeiling = linkConnectMs;
    const engineCeiling = 2 * perPidGraceMs;
    const probeCeiling = 5000;
    const splitCeiling = 3 * perPidGraceMs + linkConnectMs + 3 * perPidGraceMs;
    const meshCeiling = perPidGraceMs;
    expect(GOOSED_SIGKILL_AFTER_MS).toBeGreaterThanOrEqual(
      stdioCeiling + noticeCeiling + engineCeiling + probeCeiling + splitCeiling + meshCeiling
    );
  });
});

describe('withSystemSbin — goosed can reach lsof from its PATH', () => {
  it('appends /usr/sbin and /sbin when the PATH lacks them, keeping the existing order', () => {
    expect(withSystemSbin('/app/bin:/usr/bin:/bin', 'darwin')).toBe(
      '/app/bin:/usr/bin:/bin:/usr/sbin:/sbin'
    );
  });

  it('adds only the one that is missing', () => {
    expect(withSystemSbin('/app/bin:/usr/sbin:/usr/bin', 'darwin')).toBe(
      '/app/bin:/usr/sbin:/usr/bin:/sbin'
    );
  });

  it('leaves a PATH that already carries both untouched', () => {
    const value = '/app/bin:/usr/sbin:/sbin:/usr/bin:/bin';
    expect(withSystemSbin(value, 'linux')).toBe(value);
  });

  it('does nothing on Windows', () => {
    expect(withSystemSbin('C:\\app\\bin;C:\\Windows', 'win32')).toBe('C:\\app\\bin;C:\\Windows');
  });

  it.skipIf(process.platform === 'win32')(
    'buildGooseServeEnv hands goosed a PATH with /usr/sbin',
    () => {
      vi.stubEnv('PATH', '/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin');
      const dir = makeTempDir();
      const goose = path.join(dir, binaryName);
      fs.writeFileSync(goose, 'x');
      const env = buildGooseServeEnv('secret', goose, {});
      const entries = (env.PATH ?? '').split(path.delimiter);
      expect(entries[0]).toBe(dir);
      expect(entries).toContain('/usr/sbin');
      expect(entries).toContain('/sbin');
    }
  );
});
