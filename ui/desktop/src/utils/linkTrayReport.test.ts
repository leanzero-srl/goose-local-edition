import { describe, expect, it } from 'vitest';
import type { LinkState } from '../acp/leanzero-link';
import {
  isLinkTrayReport,
  LINK_TRAY_REASON_CHARS,
  pickLinkTrayReport,
  toLinkTrayReport,
} from './linkTrayReport';
import { linkStateSettling } from '../hooks/useLinkTrayReporter';

const signedIn = (extra: Partial<LinkState> = {}): LinkState => ({
  auth: { state: 'loggedIn', email: 'mihai@leanzero.net' },
  nodeCount: 0,
  ...extra,
});

const CONNECTED_INTENT = {
  intent: 'connected' as const,
  cause: 'userConnect' as const,
  updatedAt: '2026-09-24T10:00:00Z',
};

describe('toLinkTrayReport', () => {
  it('a failed launch reconnect is the loud line with Retry — never a silent "not connected"', () => {
    const report = toLinkTrayReport(
      signedIn({
        intent: CONNECTED_INTENT,
        lastError: 'mesh join failed: timeout',
        reconnect: { state: 'failed', reason: 'mesh join failed: timeout', at: 'x' },
      })
    );
    expect(report).toEqual({
      tone: 'failed',
      line: 'Link: reconnect failed — mesh join failed: timeout',
      action: 'connect',
      actionLabel: 'Retry',
    });
  });

  it('a reconnect that failed because the credential is gone opens the Link tab to sign in', () => {
    const report = toLinkTrayReport({
      auth: { state: 'loggedOut' },
      nodeCount: 0,
      reconnect: { state: 'failed', reason: 'not signed in', at: 'x' },
    });
    expect(report?.action).toBe('open');
    expect(report?.line).toBe('Link: reconnect failed — not signed in');
  });

  it('connected, reconnecting, disconnected and signed-out each read as themselves', () => {
    expect(
      toLinkTrayReport({
        auth: { state: 'connected', email: 'a', meshIp: '100.64.0.3' },
        nodeCount: 2,
      })?.line
    ).toBe('Link: connected · 100.64.0.3 · 2 nodes');
    expect(
      toLinkTrayReport({
        auth: { state: 'connecting', email: 'a' },
        nodeCount: 0,
        reconnect: { state: 'reconnecting', startedAt: 'x' },
      })?.line
    ).toBe('Link: reconnecting…');
    expect(
      toLinkTrayReport(
        signedIn({
          intent: { ...CONNECTED_INTENT, intent: 'disconnected', cause: 'userDisconnect' },
        })
      )
    ).toEqual({
      tone: 'off',
      line: 'Link: disconnected',
      action: 'connect',
      actionLabel: 'Connect',
    });
    expect(toLinkTrayReport({ auth: { state: 'loggedOut' }, nodeCount: 0 })).toBeNull();
  });

  it('a daemon that died under a connected intent says why and offers Retry', () => {
    const report = toLinkTrayReport(
      signedIn({ intent: CONNECTED_INTENT, lastError: 'mesh daemon died under the connection' })
    );
    expect(report?.tone).toBe('failed');
    expect(report?.line).toBe('Link: not connected — mesh daemon died under the connection');
  });

  it('an unreadable intent is named', () => {
    expect(toLinkTrayReport(signedIn({ intentError: 'malformed' }))?.line).toBe(
      'Link: intent unreadable — malformed'
    );
  });

  it('a long reason is clipped to one menu line', () => {
    const reason = 'x'.repeat(LINK_TRAY_REASON_CHARS * 2);
    const line = toLinkTrayReport(
      signedIn({ reconnect: { state: 'failed', reason, at: 'x' } })
    )?.line;
    expect(line?.endsWith('…')).toBe(true);
    expect(line!.length).toBeLessThan(LINK_TRAY_REASON_CHARS + 40);
  });
});

describe('pickLinkTrayReport (one goosed per window)', () => {
  it('the window whose backend holds the mesh speaks for the Mac', () => {
    const connected = toLinkTrayReport({
      auth: { state: 'connected', email: 'a', meshIp: '100.64.0.3' },
      nodeCount: 2,
    });
    const sibling = toLinkTrayReport(
      signedIn({
        intent: CONNECTED_INTENT,
        reconnect: { state: 'skipped', reason: 'another goose on this Mac already holds the mesh' },
      })
    );
    expect(sibling?.line).toBe(
      'Link: not reconnected here — another goose on this Mac already holds the mesh'
    );
    expect(pickLinkTrayReport([sibling, connected])).toBe(connected);
    expect(pickLinkTrayReport([connected, sibling])).toBe(connected);
    expect(pickLinkTrayReport([null, null])).toBeNull();
    const failed = toLinkTrayReport(
      signedIn({ reconnect: { state: 'failed', reason: 'r', at: 'x' } })
    );
    expect(pickLinkTrayReport([sibling, failed])).toBe(failed);
  });

  it('a never-connected sign-in reads "not connected", not "disconnected"', () => {
    expect(
      toLinkTrayReport(
        signedIn({ intent: { intent: 'disconnected', cause: 'noRecord', updatedAt: 'x' } })
      )?.line
    ).toBe('Link: not connected');
  });
});

describe('isLinkTrayReport (the IPC trust boundary)', () => {
  it('accepts every report the projection makes and null', () => {
    expect(isLinkTrayReport(null)).toBe(true);
    expect(
      isLinkTrayReport(
        toLinkTrayReport(signedIn({ reconnect: { state: 'failed', reason: 'r', at: 'x' } }))
      )
    ).toBe(true);
  });

  it('refuses unknown tones/actions and a label without an action', () => {
    expect(isLinkTrayReport({ tone: 'red', line: 'x', action: null, actionLabel: null })).toBe(
      false
    );
    expect(isLinkTrayReport({ tone: 'ok', line: 'x', action: 'rm -rf', actionLabel: 'Go' })).toBe(
      false
    );
    expect(isLinkTrayReport({ tone: 'ok', line: 'x', action: null, actionLabel: 'Go' })).toBe(
      false
    );
    expect(isLinkTrayReport('Link: ok')).toBe(false);
  });
});

describe('linkStateSettling', () => {
  it('keeps reading while a launch reconnect has no outcome yet, and stops once it has one', () => {
    expect(
      linkStateSettling(signedIn({ intent: CONNECTED_INTENT, reconnect: { state: 'idle' } }))
    ).toBe(true);
    expect(linkStateSettling({ auth: { state: 'connecting', email: 'a' }, nodeCount: 0 })).toBe(
      true
    );
    expect(
      linkStateSettling(
        signedIn({
          intent: CONNECTED_INTENT,
          reconnect: { state: 'failed', reason: 'r', at: 'x' },
        })
      )
    ).toBe(false);
    expect(
      linkStateSettling(
        signedIn({
          intent: { ...CONNECTED_INTENT, intent: 'disconnected', cause: 'userDisconnect' },
          reconnect: { state: 'idle' },
        })
      )
    ).toBe(false);
  });
});
