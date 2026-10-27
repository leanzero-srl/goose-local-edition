import { app } from 'electron';
import { spawn } from 'node:child_process';

export function registerGooseProtocolClient(development: boolean): void {
  // Isolated dev launches must not replace the installed app's OS-wide deep-link handler.
  if (development && !app.isPackaged && process.env.GOOSE_SKIP_PROTOCOL_REGISTRATION === '1') {
    console.log('[Main] Development mode: Skipping protocol registration for goose://');
    return;
  }

  if (development) {
    console.log('[Main] Development mode: Forcing protocol registration for goose://');
  }
  app.setAsDefaultProtocolClient('goose');

  if (development && process.platform === 'darwin') {
    try {
      spawn('open', ['-a', process.execPath, '--args', '--reset-protocol-handler', 'goose'], {
        detached: true,
        stdio: 'ignore',
      });
    } catch {
      console.warn('[Main] Could not reset protocol handler');
    }
  }
}
