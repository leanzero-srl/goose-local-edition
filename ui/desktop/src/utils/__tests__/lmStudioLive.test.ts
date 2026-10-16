import net from 'node:net';
import { describe, expect, it, vi } from 'vitest';
import {
  fleetStatusHandler,
  lmStudioHome,
  lmStudioLiveness,
  parseLmsPs,
  pidAliveViaSignalZero,
  tcpAcceptsViaNet,
  type FleetStatusDeps,
  type LmStudioLivenessDeps,
} from '../lmStudioLive';

const HOME = '/Users/gabee';
const SERVICE_FILE = '/Users/gabee/.lmstudio/.internal/http-server.json';
/** The file as LM Studio writes it (read from this laptop's running service, 2026-10-16). */
const SERVICE_JSON = JSON.stringify({ host: '127.0.0.1', pid: 11767, port: 41343 });

const deps = (over: Partial<LmStudioLivenessDeps> = {}): LmStudioLivenessDeps => ({
  home: HOME,
  readFile: (p) => (p === SERVICE_FILE ? SERVICE_JSON : null),
  pidAlive: () => true,
  tcpAccepts: async () => true,
  ...over,
});

describe('lmStudioLiveness: is a service already up, decided without spawning anything', () => {
  it('a service whose pid is alive and whose port accepts is live', async () => {
    expect(await lmStudioLiveness(deps())).toEqual({
      live: true,
      host: '127.0.0.1',
      pid: 11767,
      port: 41343,
    });
  });

  it('no http-server.json means nothing is running', async () => {
    expect(await lmStudioLiveness(deps({ readFile: () => null }))).toEqual({
      live: false,
      reason: 'no-service-file',
    });
  });

  it('a file that does not parse, or lacks pid/port, is not proof of life', async () => {
    expect(await lmStudioLiveness(deps({ readFile: () => '{not json' }))).toEqual({
      live: false,
      reason: 'bad-service-file',
    });
    expect(
      await lmStudioLiveness(
        deps({ readFile: () => JSON.stringify({ host: '127.0.0.1', port: 41343 }) })
      )
    ).toEqual({ live: false, reason: 'bad-service-file' });
  });

  // The measured case: the user quit LM Studio, the file stays behind naming a pid that is gone.
  it('a stale file whose pid is gone is not running, and the port is never touched', async () => {
    const tcpAccepts = vi.fn(async () => true);
    expect(await lmStudioLiveness(deps({ pidAlive: () => false, tcpAccepts }))).toEqual({
      live: false,
      reason: 'pid-gone',
    });
    expect(tcpAccepts).not.toHaveBeenCalled();
  });

  it('a reused pid with nothing listening on the port is not running', async () => {
    const tcpAccepts = vi.fn(async () => false);
    expect(await lmStudioLiveness(deps({ tcpAccepts }))).toEqual({
      live: false,
      reason: 'port-closed',
    });
    expect(tcpAccepts).toHaveBeenCalledWith('127.0.0.1', 41343);
  });

  it('follows ~/.lmstudio-home-pointer to a relocated LM Studio home', async () => {
    const files: Record<string, string> = {
      '/Users/gabee/.lmstudio-home-pointer': '/Volumes/Models/lmstudio\n',
      '/Volumes/Models/lmstudio/.internal/http-server.json': SERVICE_JSON,
    };
    const readFile = (p: string) => files[p] ?? null;
    expect(lmStudioHome(HOME, readFile)).toBe('/Volumes/Models/lmstudio');
    expect((await lmStudioLiveness(deps({ readFile }))).live).toBe(true);
  });
});

describe('pidAliveViaSignalZero', () => {
  const err = (code: string) => Object.assign(new Error(code), { code });

  it('signal 0 delivered: alive', () => {
    const kill = vi.fn();
    expect(pidAliveViaSignalZero(kill)(42)).toBe(true);
    expect(kill).toHaveBeenCalledWith(42, 0);
  });

  it('ESRCH: gone; EPERM: alive under another user', () => {
    expect(
      pidAliveViaSignalZero(() => {
        throw err('ESRCH');
      })(42)
    ).toBe(false);
    expect(
      pidAliveViaSignalZero(() => {
        throw err('EPERM');
      })(42)
    ).toBe(true);
  });
});

describe('tcpAcceptsViaNet: a real loopback connect', () => {
  it('true for a listening port, false once it is closed', async () => {
    const server = net.createServer((s) => s.destroy());
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', () => r()));
    const port = (server.address() as net.AddressInfo).port;
    const probe = tcpAcceptsViaNet(500);
    expect(await probe('127.0.0.1', port)).toBe(true);
    await new Promise<void>((r) => server.close(() => r()));
    expect(await probe('127.0.0.1', port)).toBe(false);
  });
});

describe('fleetStatusHandler: `lms ps` only for an enabled surface over a running service', () => {
  const handler = (over: Partial<FleetStatusDeps> = {}) => {
    const d = {
      surfaceEnabled: vi.fn(() => true),
      resolveBin: vi.fn(() => '/Users/gabee/.lmstudio/bin/lms'),
      liveness: vi.fn(async () => ({
        live: true as const,
        host: '127.0.0.1',
        pid: 11767,
        port: 41343,
      })),
      runPs: vi.fn(async () => ({ 'gabee-qwen3.6-27b': 'idle' })),
      log: vi.fn(),
      ...over,
    };
    return { d, poll: fleetStatusHandler(d) };
  };

  it('surface off (the default install): no lookup, no liveness probe, no spawn, the honest {}', async () => {
    const { d, poll } = handler({ surfaceEnabled: vi.fn(() => false) });
    for (let tick = 0; tick < 40; tick++) expect(await poll()).toEqual({});
    expect(d.resolveBin).not.toHaveBeenCalled();
    expect(d.liveness).not.toHaveBeenCalled();
    expect(d.runPs).not.toHaveBeenCalled();
    expect(d.log).toHaveBeenCalledTimes(1);
  });

  it('no lms binary: no liveness probe, no spawn', async () => {
    const { d, poll } = handler({ resolveBin: vi.fn(() => null) });
    expect(await poll()).toEqual({});
    expect(d.liveness).not.toHaveBeenCalled();
    expect(d.runPs).not.toHaveBeenCalled();
  });

  // Gabee's Mac 2026-10-16, replayed: lms installed, LM Studio quit by the user (stale service file),
  // the panel polling every 1.5 s. Before, each tick ran `lms ps`, which relaunched the service.
  it('surface on, lms installed, LM Studio quit: 40 ticks spawn nothing and log once', async () => {
    const { d, poll } = handler({
      liveness: vi.fn(async () => ({ live: false as const, reason: 'pid-gone' as const })),
    });
    for (let tick = 0; tick < 40; tick++) expect(await poll()).toEqual({});
    expect(d.liveness).toHaveBeenCalledTimes(40);
    expect(d.runPs).not.toHaveBeenCalled();
    expect(d.log).toHaveBeenCalledTimes(1);
    expect(d.log).toHaveBeenCalledWith(expect.stringContaining('pid-gone'));
  });

  it('a running service is polled, and a state change is logged once more', async () => {
    let live = false;
    const { d, poll } = handler({
      liveness: vi.fn(async () =>
        live
          ? { live: true as const, host: '127.0.0.1', pid: 11767, port: 41343 }
          : { live: false as const, reason: 'no-service-file' as const }
      ),
    });
    expect(await poll()).toEqual({});
    live = true;
    expect(await poll()).toEqual({ 'gabee-qwen3.6-27b': 'idle' });
    expect(await poll()).toEqual({ 'gabee-qwen3.6-27b': 'idle' });
    expect(d.runPs).toHaveBeenCalledTimes(2);
    expect(d.runPs).toHaveBeenCalledWith('/Users/gabee/.lmstudio/bin/lms');
    expect(d.log).toHaveBeenCalledTimes(2);
  });

  it('the setting is read per poll: switching the surface off stops the spawns at once', async () => {
    let on = true;
    const { d, poll } = handler({ surfaceEnabled: vi.fn(() => on) });
    await poll();
    on = false;
    await poll();
    await poll();
    expect(d.runPs).toHaveBeenCalledTimes(1);
  });
});

describe('parseLmsPs', () => {
  it('maps identifier to status and skips rows missing either', () => {
    expect(
      parseLmsPs(
        JSON.stringify([
          { identifier: 'gabee-qwen3.6-27b', status: 'generating' },
          { identifier: 'no-status' },
          { status: 'idle' },
        ])
      )
    ).toEqual({ 'gabee-qwen3.6-27b': 'generating' });
  });
});
