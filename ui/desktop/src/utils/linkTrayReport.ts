import type { LinkState } from '../acp/leanzero-link';

/**
 * What the renderer hands MAIN about LeanZero Link after every state read (main owns no ACP
 * client): one tray line and the one action it offers. Validated on arrival — IPC is a trust
 * boundary. `null` = nothing to say (signed out and nothing failed).
 *
 * The loud case is a launch reconnect that did not bring the mesh back: before goosed honoured
 * the persisted intent, every relaunch came back "not connected" with no word anywhere, and the
 * features that need the peer degraded silently. That state now reads
 * "Link: reconnect failed — <reason>" with Retry.
 */
export type LinkTrayTone = 'ok' | 'busy' | 'off' | 'failed';
/** `connect` calls the same Connect the Link tab does; `open` shows the Link tab. */
export type LinkTrayAction = 'connect' | 'open';

export interface LinkTrayReport {
  tone: LinkTrayTone;
  line: string;
  action: LinkTrayAction | null;
  /** The action's menu label ("Retry", "Connect", "Open LeanZero Link"). */
  actionLabel: string | null;
}

/** A menu item label must stay one readable line; the full text lives in the Link tab. */
export const LINK_TRAY_REASON_CHARS = 140;

function clip(text: string): string {
  const one = text.replace(/\s+/g, ' ').trim();
  return one.length > LINK_TRAY_REASON_CHARS ? `${one.slice(0, LINK_TRAY_REASON_CHARS - 1)}…` : one;
}

export function toLinkTrayReport(state: LinkState | null): LinkTrayReport | null {
  if (!state) return null;
  const { auth, reconnect, intent, intentError, lastError } = state;

  if (auth.state === 'connected') {
    const nodes = `${state.nodeCount} node${state.nodeCount === 1 ? '' : 's'}`;
    return {
      tone: 'ok',
      line: `Link: connected · ${auth.meshIp} · ${nodes}`,
      action: null,
      actionLabel: null,
    };
  }
  if (auth.state === 'connecting') {
    return {
      tone: 'busy',
      line: reconnect?.state === 'reconnecting' ? 'Link: reconnecting…' : 'Link: connecting…',
      action: null,
      actionLabel: null,
    };
  }
  if (reconnect?.state === 'failed') {
    const signedIn = auth.state === 'loggedIn';
    return {
      tone: 'failed',
      line: `Link: reconnect failed — ${clip(reconnect.reason)}`,
      action: signedIn ? 'connect' : 'open',
      actionLabel: signedIn ? 'Retry' : 'Open LeanZero Link',
    };
  }
  if (auth.state !== 'loggedIn') return null;
  if (intentError) {
    return {
      tone: 'failed',
      line: `Link: intent unreadable — ${clip(intentError)}`,
      action: 'open',
      actionLabel: 'Open LeanZero Link',
    };
  }
  if (reconnect?.state === 'skipped' && intent?.intent === 'connected') {
    // Another goose on this Mac (a second window's backend) holds the mesh.
    return {
      tone: 'off',
      line: `Link: not reconnected here — ${clip(reconnect.reason)}`,
      action: null,
      actionLabel: null,
    };
  }
  if (intent?.cause === 'userDisconnect') {
    return { tone: 'off', line: 'Link: disconnected', action: 'connect', actionLabel: 'Connect' };
  }
  if (lastError) {
    return {
      tone: 'failed',
      line: `Link: not connected — ${clip(lastError)}`,
      action: 'connect',
      actionLabel: 'Retry',
    };
  }
  return { tone: 'off', line: 'Link: not connected', action: 'connect', actionLabel: 'Connect' };
}

const TONES: readonly string[] = ['ok', 'busy', 'off', 'failed'];

/**
 * Every window runs its own goosed, and each reports. The tray shows ONE line: the window whose
 * backend holds the mesh speaks for the Mac (connected > connecting > a failure > off), so a
 * second window's "not reconnected here" never overwrites a live connection.
 */
const TONE_RANK: Record<LinkTrayTone, number> = { ok: 3, busy: 2, failed: 1, off: 0 };

export function pickLinkTrayReport(
  reports: Iterable<LinkTrayReport | null>
): LinkTrayReport | null {
  let best: LinkTrayReport | null = null;
  for (const report of reports) {
    if (report && (!best || TONE_RANK[report.tone] > TONE_RANK[best.tone])) best = report;
  }
  return best;
}
const ACTIONS: readonly string[] = ['connect', 'open'];

export function isLinkTrayReport(value: unknown): value is LinkTrayReport | null {
  if (value === null) return true;
  if (typeof value !== 'object' || value == null) return false;
  const r = value as Record<string, unknown>;
  const action = r.action;
  const label = r.actionLabel;
  return (
    typeof r.tone === 'string' &&
    TONES.includes(r.tone) &&
    typeof r.line === 'string' &&
    (action === null || (typeof action === 'string' && ACTIONS.includes(action))) &&
    (label === null || typeof label === 'string') &&
    (action === null) === (label === null)
  );
}
