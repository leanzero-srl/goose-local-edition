import { benchModelShortName } from './benchTray';

/**
 * The distributed run's status on every Mac taking part (owner 2026-10-31: "when distributed
 * compute happens I want to see the same message in the status bar for all macs involved").
 *
 * The CONTROLLER's main writes the title it sets to `controller.json`; goosed's supervisor relays
 * it, with the run's own facts, to each peer as `rank-status.json` (crates/goose-sidecar
 * distributed/peer_tray.rs, wire fixture `peerTray.fixture.json`). A PEER's main reads that record
 * and shows the same title with its role added. A record that went quiet, or whose rank is gone,
 * says the rank stopped for a short hold and then clears — never a frozen message.
 */
export const PEER_TRAY_DIR = ['.goose', 'distributed', 'tray'] as const;
export const CONTROLLER_TRAY_FILE = 'controller.json';
export const RANK_STATUS_FILE = 'rank-status.json';
/** How often a peer reads its record and the controller rewrites its title (the relay's window
 * is ≥ 6 s, so a title rewritten every 3 s is never read as stale). */
export const PEER_TRAY_POLL_MS = 2000;
export const CONTROLLER_TRAY_HEARTBEAT_MS = 3000;
/** How long "rank stopped" stays before the tray clears. */
export const PEER_TRAY_HOLD_MS = 30_000;

export interface PeerTrayRecord {
  v: number;
  controller: string;
  rank: number;
  size: number;
  modelId: string | null;
  state: string;
  loadPhase: string | null;
  loadedBytes: number | null;
  plannedBytes: number | null;
  title: string | null;
  lines: string[];
  error: string | null;
  rankPid: number | null;
  updatedMs: number;
  staleAfterS: number;
}

export interface PeerTrayView {
  title: string;
  lines: string[];
}

export interface PeerTrayRead {
  view: PeerTrayView | null;
  /** The record has said all it will: the reader removes the file. */
  expired: boolean;
}

export interface ControllerTrayPayload {
  title: string;
  lines: string[];
  updatedMs: number;
}

const isCount = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value) && value >= 0;
const textOrNull = (value: unknown): string | null =>
  typeof value === 'string' && value.trim() !== '' ? value : null;

/** The record as the relay writes it, or null for anything else (a half-written or foreign file). */
export function parsePeerTrayRecord(text: string): PeerTrayRecord | null {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof raw !== 'object' || raw === null) return null;
  const r = raw as Record<string, unknown>;
  if (r.v !== 1) return null;
  if (typeof r.controller !== 'string' || typeof r.state !== 'string') return null;
  if (!isCount(r.rank) || !isCount(r.size) || !isCount(r.updatedMs) || !isCount(r.staleAfterS))
    return null;
  return {
    v: 1,
    controller: r.controller,
    rank: r.rank,
    size: r.size,
    modelId: textOrNull(r.modelId),
    state: r.state,
    loadPhase: textOrNull(r.loadPhase),
    loadedBytes: isCount(r.loadedBytes) ? r.loadedBytes : null,
    plannedBytes: isCount(r.plannedBytes) ? r.plannedBytes : null,
    title: textOrNull(r.title),
    lines: Array.isArray(r.lines)
      ? r.lines.filter((line): line is string => typeof line === 'string')
      : [],
    error: textOrNull(r.error),
    rankPid: isCount(r.rankPid) ? r.rankPid : null,
    updatedMs: r.updatedMs,
    staleAfterS: r.staleAfterS,
  };
}

const gb = (bytes: number) => (bytes / 1024 ** 3).toFixed(1);

/** The run's own words for the title when the controller's app sent none (no app, or its title
 * went stale): what the supervisor knows. */
function stateWords(record: PeerTrayRecord): string {
  if (record.state === 'starting' || record.loadPhase === 'loading') {
    if (record.loadPhase === 'warming') return 'warming up';
    if (record.loadPhase === 'ready') return 'loaded';
    const progress =
      record.loadedBytes != null && record.plannedBytes
        ? ` ${gb(record.loadedBytes)}/${gb(record.plannedBytes)} GB`
        : '';
    return `loading shards${progress}`;
  }
  return record.state;
}

function role(record: PeerTrayRecord): string {
  return `rank ${record.rank}/${record.size} (for ${record.controller})`;
}

/**
 * What this Mac's tray shows for the record at `nowMs`. `rankGoneSinceMs` is when this Mac first
 * saw the record's rank pid gone (null while it runs, or when the record names no pid).
 */
export function peerTrayView(
  record: PeerTrayRecord,
  nowMs: number,
  rankGoneSinceMs: number | null,
  holdMs: number = PEER_TRAY_HOLD_MS
): PeerTrayRead {
  const staleAtMs = record.updatedMs + record.staleAfterS * 1000;
  const ends = [
    record.state === 'stopped' || record.state === 'failed' ? record.updatedMs : null,
    nowMs > staleAtMs && rankGoneSinceMs != null ? Math.max(staleAtMs, rankGoneSinceMs) : null,
    nowMs > staleAtMs && record.rankPid == null ? staleAtMs : null,
    rankGoneSinceMs != null && record.state !== 'starting' ? rankGoneSinceMs : null,
  ].filter((at): at is number => at != null);
  const header = `Rank ${record.rank} of ${record.size} for ${record.controller}`;
  if (ends.length > 0) {
    const endedAtMs = Math.min(...ends);
    if (nowMs - endedAtMs > holdMs) return { view: null, expired: true };
    const failed = record.state === 'failed';
    return {
      view: {
        title: failed ? `Split failed · ${role(record)}` : `Rank ${record.rank}/${record.size} stopped (for ${record.controller})`,
        lines: [header, ...(failed && record.error ? [`Error: ${record.error}`] : [])],
      },
      expired: false,
    };
  }
  if (nowMs > staleAtMs) {
    // The controller went quiet while this Mac's rank still runs: say so, never the old title.
    const quietS = Math.round((nowMs - record.updatedMs) / 1000);
    return {
      view: {
        title: `Rank ${record.rank}/${record.size} · no word from ${record.controller}`,
        lines: [header, `Last status ${quietS} s ago`],
      },
      expired: false,
    };
  }
  const base =
    record.title ??
    ['Split', record.modelId ? benchModelShortName(record.modelId) : null, stateWords(record)]
      .filter((part): part is string => part != null)
      .join(' · ');
  const lines =
    record.lines.length > 0
      ? record.lines
      : record.modelId
        ? [`Model: ${record.modelId}`]
        : [];
  return {
    view: {
      title: `${base} · ${role(record)}`,
      lines: [header, ...lines, ...(record.error ? [`Error: ${record.error}`] : [])],
    },
    expired: false,
  };
}

/** What the controller's main writes while a distributed run or a benchmark holds its title;
 * null = nothing to relay (the file is removed). */
export function controllerTrayPayload(
  title: string,
  lines: string[],
  active: boolean,
  nowMs: number
): ControllerTrayPayload | null {
  if (!active || title.trim() === '') return null;
  return { title, lines, updatedMs: nowMs };
}
