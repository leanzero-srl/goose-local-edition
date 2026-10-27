import { app } from 'electron';
import { spawn } from 'node:child_process';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { registerGooseProtocolClient } from './protocolRegistration';

const { electronApp } = vi.hoisted(() => ({
  electronApp: { isPackaged: false, setAsDefaultProtocolClient: vi.fn() },
}));
vi.mock('electron', () => ({ app: electronApp }));
vi.mock('node:child_process', () => {
  const mock = { spawn: vi.fn() };
  return { ...mock, default: mock };
});

beforeEach(() => {
  vi.clearAllMocks();
  electronApp.isPackaged = false;
  vi.stubEnv('GOOSE_SKIP_PROTOCOL_REGISTRATION', undefined);
  vi.stubGlobal('process', Object.create(process, { platform: { value: 'darwin' } }));
  vi.spyOn(console, 'log').mockImplementation(() => {});
  vi.spyOn(console, 'warn').mockImplementation(() => {});
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
  vi.restoreAllMocks();
});

describe('goose protocol registration', () => {
  it('suppresses registration and the reset subprocess for an explicitly isolated dev launch', () => {
    vi.stubEnv('GOOSE_SKIP_PROTOCOL_REGISTRATION', '1');
    registerGooseProtocolClient(true);
    expect(app.setAsDefaultProtocolClient).not.toHaveBeenCalled();
    expect(spawn).not.toHaveBeenCalled();
  });

  it.each([undefined, '', '0', 'true'])(
    'preserves normal dev registration with flag %s',
    (flag) => {
      vi.stubEnv('GOOSE_SKIP_PROTOCOL_REGISTRATION', flag);
      registerGooseProtocolClient(true);
      expect(app.setAsDefaultProtocolClient).toHaveBeenCalledExactlyOnceWith('goose');
      expect(spawn).toHaveBeenCalledExactlyOnceWith(
        'open',
        ['-a', process.execPath, '--args', '--reset-protocol-handler', 'goose'],
        { detached: true, stdio: 'ignore' }
      );
    }
  );

  it.each([false, true])('cannot disable packaged registration (dev server: %s)', (development) => {
    electronApp.isPackaged = true;
    vi.stubEnv('GOOSE_SKIP_PROTOCOL_REGISTRATION', '1');
    registerGooseProtocolClient(development);
    expect(app.setAsDefaultProtocolClient).toHaveBeenCalledExactlyOnceWith('goose');
    expect(spawn).toHaveBeenCalledTimes(development ? 1 : 0);
  });

  it('does not opt out without a dev server even when unpackaged', () => {
    vi.stubEnv('GOOSE_SKIP_PROTOCOL_REGISTRATION', '1');
    registerGooseProtocolClient(false);
    expect(app.setAsDefaultProtocolClient).toHaveBeenCalledExactlyOnceWith('goose');
    expect(spawn).not.toHaveBeenCalled();
  });

  it.each(['linux', 'win32'])('registers without a macOS reset on %s', (platform) => {
    vi.stubGlobal('process', Object.create(process, { platform: { value: platform } }));
    registerGooseProtocolClient(true);
    expect(app.setAsDefaultProtocolClient).toHaveBeenCalledExactlyOnceWith('goose');
    expect(spawn).not.toHaveBeenCalled();
  });

  it('retains the warning when the reset subprocess cannot start', () => {
    vi.mocked(spawn).mockImplementationOnce(() => {
      throw new Error('spawn failed');
    });
    registerGooseProtocolClient(true);
    expect(app.setAsDefaultProtocolClient).toHaveBeenCalledExactlyOnceWith('goose');
    expect(console.warn).toHaveBeenCalledExactlyOnceWith('[Main] Could not reset protocol handler');
  });
});
