import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import type { EnginePhase } from '../lz/tokens';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import { peerRefuses, type Mac } from './macs';
import { activityPhase, hostingPhase, runPhase } from './mlxPhase';
import type { MlxActivity } from './mlxLiveStats';

/**
 * What one Mac is doing, in ONE vocabulary and the engine-phase palette — the My Macs card and the
 * menu-bar tray say exactly this, so a Mac never reads two ways. `phase` null = nothing an engine
 * could be doing is known (offline, switched off, not read yet): no palette colour is claimed.
 */

export type MacState =
  | 'offline'
  | 'off'
  | 'checking'
  | 'unreadable'
  | 'notLoaded'
  | 'loading'
  | 'idle'
  | 'reading'
  | 'writing'
  | 'queued'
  | 'failed'
  | 'split'
  | 'hosting';

export interface MacSummary {
  phase: EnginePhase | null;
  state: MacState;
  /** The model in memory (or loading, or failed), full id. */
  modelId: string | null;
  decodeTps: number | null;
  /** The reason behind `unreadable` / `failed`, goose's words. */
  detail: string | null;
  /** A split: how many Macs; hosting: whose split. */
  splitCount?: number;
  requester?: string;
}

export interface SummaryInput {
  status: MlxEngineStatus | null;
  statusError: string | null;
  activity: MlxActivity | null;
  decodeTps: number | null;
  /** This Mac only: the distributed engine it supervises (or null when not read). */
  distributed?: MlxDistributedStatus | null;
}

function fromActivity(activity: MlxActivity | null): { phase: EnginePhase; state: MacState } {
  if (!activity) return { phase: 'idle', state: 'idle' };
  const phase = activityPhase(activity);
  switch (activity) {
    case 'generating':
      return { phase, state: 'writing' };
    case 'prefill':
      return { phase, state: 'reading' };
    case 'queued':
      return { phase, state: 'queued' };
    case 'not_loaded':
      return { phase, state: 'notLoaded' };
    default:
      return { phase, state: 'idle' };
  }
}

export function summarizeMac(mac: Mac, input: SummaryInput): MacSummary {
  const blank = { modelId: null, decodeTps: null, detail: null };
  if (!mac.online) return { ...blank, phase: null, state: 'offline', detail: mac.pollError };
  if (peerRefuses(mac, 'manage')) return { ...blank, phase: null, state: 'off' };
  const dist = mac.isSelf ? (input.distributed ?? null) : null;
  if (dist?.mode === 'distributed') {
    return {
      ...blank,
      phase: runPhase(dist.state, dist.admissionOpen),
      state: 'split',
      modelId: dist.modelId ?? null,
      splitCount: dist.nodes.length,
      detail: dist.lastError ?? null,
    };
  }
  const hosting = dist?.hosting ?? input.status?.hosting ?? null;
  if (hosting) {
    return {
      ...blank,
      phase: hostingPhase(hosting.state),
      state: 'hosting',
      modelId: hosting.modelId,
      requester: hosting.requesterName,
    };
  }
  const status = input.status;
  if (!status) {
    return input.statusError
      ? { ...blank, phase: 'failed', state: 'unreadable', detail: input.statusError }
      : { ...blank, phase: null, state: 'checking' };
  }
  const modelId = status.modelId ?? null;
  switch (status.state) {
    case 'stopped':
      return { ...blank, phase: 'unloaded', state: 'notLoaded' };
    case 'mounting':
      return { ...blank, phase: 'loading', state: 'loading', modelId };
    case 'failed':
      return {
        ...blank,
        phase: 'failed',
        state: 'failed',
        modelId,
        detail: status.lastError ?? null,
      };
    default: {
      const { phase, state } = fromActivity(mac.isSelf ? input.activity : null);
      return {
        ...blank,
        phase,
        state,
        modelId,
        decodeTps: state === 'writing' && input.decodeTps ? input.decodeTps : null,
      };
    }
  }
}

const WORDS = defineMessages({
  offline: { id: 'macs.state.offline', defaultMessage: 'Offline' },
  off: { id: 'macs.state.off', defaultMessage: 'Off' },
  checking: { id: 'macs.state.checking', defaultMessage: 'Checking' },
  unreadable: { id: 'macs.state.unreadable', defaultMessage: 'Can’t read' },
  notLoaded: { id: 'macs.state.notLoaded', defaultMessage: 'Not loaded' },
  loading: { id: 'macs.state.loading', defaultMessage: 'Loading' },
  idle: { id: 'macs.state.idle', defaultMessage: 'Idle' },
  reading: { id: 'macs.state.reading', defaultMessage: 'Reading' },
  writing: { id: 'macs.state.writing', defaultMessage: 'Writing' },
  queued: { id: 'macs.state.queued', defaultMessage: 'Queued' },
  failed: { id: 'macs.state.failed', defaultMessage: 'Failed' },
  split: { id: 'macs.state.split', defaultMessage: 'Split' },
  hosting: { id: 'macs.state.hosting', defaultMessage: 'Part of a split' },
});

const LINES = defineMessages({
  offline: { id: 'macs.line.offline', defaultMessage: 'Not reachable over LeanZero Link' },
  checking: { id: 'macs.line.checking', defaultMessage: 'Reading its engine…' },
  notLoaded: { id: 'macs.line.notLoaded', defaultMessage: 'No model loaded' },
  loading: { id: 'macs.line.loading', defaultMessage: 'Loading {model}' },
  running: { id: 'macs.line.running', defaultMessage: '{model}' },
  writing: { id: 'macs.line.writing', defaultMessage: '{model} · {tps} tok/s' },
  failed: { id: 'macs.line.failed', defaultMessage: '{model} failed: {reason}' },
  failedNoModel: { id: 'macs.line.failedNoModel', defaultMessage: 'The engine failed: {reason}' },
  split: {
    id: 'macs.line.split',
    defaultMessage: '{model} · split across {count, plural, one {# Mac} other {# Macs}}',
  },
  hosting: { id: 'macs.line.hosting', defaultMessage: '{model} · part of {requester}’s split' },
});

export function macStateWord(intl: IntlShape, state: MacState): string {
  return intl.formatMessage(WORDS[state]);
}

/** The model's own name: the last segment of its id (`Mihai-LeanZero/Qwen3.8-27B` → `Qwen3.8-27B`). */
export function shortModel(modelId: string): string {
  return modelId.split('/').pop() || modelId;
}

function rate(tps: number): string {
  return tps >= 100 ? tps.toFixed(0) : tps.toFixed(1);
}

/**
 * The one line under a Mac's name. `off` / `unreadable` carry their own sentence (`detail` or the
 * caller's), so this returns null for them — the caller shows that sentence in its tone.
 */
export function macLine(intl: IntlShape, summary: MacSummary): string | null {
  const model = summary.modelId ? shortModel(summary.modelId) : '—';
  switch (summary.state) {
    case 'offline':
      return intl.formatMessage(LINES.offline);
    case 'checking':
      return intl.formatMessage(LINES.checking);
    case 'notLoaded':
      return intl.formatMessage(LINES.notLoaded);
    case 'loading':
      return intl.formatMessage(LINES.loading, { model });
    case 'failed':
      return summary.modelId
        ? intl.formatMessage(LINES.failed, { model, reason: summary.detail ?? '—' })
        : intl.formatMessage(LINES.failedNoModel, { reason: summary.detail ?? '—' });
    case 'split':
      return intl.formatMessage(LINES.split, { model, count: summary.splitCount ?? 0 });
    case 'hosting':
      return intl.formatMessage(LINES.hosting, { model, requester: summary.requester ?? '—' });
    case 'writing':
      return summary.decodeTps != null
        ? intl.formatMessage(LINES.writing, { model, tps: rate(summary.decodeTps) })
        : intl.formatMessage(LINES.running, { model });
    case 'idle':
    case 'reading':
    case 'queued':
      return intl.formatMessage(LINES.running, { model });
    case 'off':
    case 'unreadable':
      return null;
  }
}
