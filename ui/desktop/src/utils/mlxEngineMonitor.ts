import * as yaml from 'yaml';
import {
  EMPTY_BOOK,
  advanceRateBook,
  parseMlxLiveStatus,
  type RateBook,
  type MlxLiveStats,
} from '../components/leanzero-swarm/mlxLiveStats';
import type { MlxLiveStatusResult } from './mlxLiveStatus';
import { remoteLiveBase } from './mlxRemoteReport';
import {
  attributeServing,
  servingRowsForEngine,
  type MlxServing,
  type MlxServingRead,
} from './mlxServing';

/**
 * MAIN's one read loop over the local LeanZero MLX engine, for the surfaces that must not depend on
 * the Providers view being open: the menu-bar tray, and the state tile's "who is using it" line.
 *
 * Every tick is one GET of the engine's own `/v1/status` and — only while the engine has requests —
 * one GET of each goose backend's `/mlx-engine/serving`. The loop runs while the engine answers (or
 * the supervising goose says it is mounting) and STOPS the moment it does not: nothing polls an
 * engine that is not there. It is woken by the facts that can start one — a renderer's ACP status
 * read (`mlx-engine-report`, sent from `mlxEngineStatus`), the tray menu opening, the tray being
 * created — never by a clock of its own.
 *
 * Mode truth: the engine answering IS running. A refused connection is the engine gone (`off`),
 * unless goose last said it is mounting (the port opens only at the end of a mount) or failed.
 * A timeout is NOT the engine gone — a busy engine can be slow to answer — so the mode holds and
 * the reason is carried; a body that is not Rapid-MLX's makes the mode `unknown`, never `running`.
 *
 * A ROUTE to a linked Mac is read as that Mac's engine for as long as the route is published, in
 * every state: `reconnecting` while the relay read fails (or the route itself says so) — never a
 * fall back to reading THIS Mac's engine, which serves nothing while the route stands (Q-48: the
 * relaunch recording's tray read "single/off unreachable" at 19.9 s while chat still went to the
 * Studio).
 */

export type MlxEngineMode = 'unknown' | 'off' | 'mounting' | 'running' | 'failed' | 'reconnecting';

/** What a renderer's ACP `mlxEngine/status` read said (the supervising goose's own word). */
export interface MlxEngineReport {
  state: 'stopped' | 'mounting' | 'running' | 'failed';
  baseUrl?: string;
  modelId?: string;
  servedModelId?: string;
  lastError?: string;
}

const REPORT_STATES = new Set(['stopped', 'mounting', 'running', 'failed']);
const REPORT_TEXT_FIELDS = ['baseUrl', 'modelId', 'servedModelId', 'lastError'] as const;

/** An IPC payload is a report only if every field is what `mlxEngineStatus` sends. */
export function isMlxEngineReport(value: unknown): value is MlxEngineReport {
  if (value == null || typeof value !== 'object') return false;
  const r = value as Record<string, unknown>;
  if (typeof r.state !== 'string' || !REPORT_STATES.has(r.state)) return false;
  return REPORT_TEXT_FIELDS.every((k) => r[k] === undefined || typeof r[k] === 'string');
}

export interface MlxEngineSnapshot {
  /**
   * Which engine this read is of: the single engine, the distributed run's rank 0 while that run
   * owns this Mac and is up, or — while this Mac's chat is routed to a linked Mac — that Mac's
   * single engine through goosed's loopback relay. All answer `/v1/status` in the single engine's
   * shape.
   */
  engine: 'single' | 'distributed' | 'remote';
  mode: MlxEngineMode;
  /** The id the engine serves (its own `/v1/status` model, else goose's served id, else the HF id). */
  modelId: string | null;
  baseUrl: string | null;
  stats: MlxLiveStats | null;
  /** Why `stats` is absent or stale, verbatim. */
  statusDetail: string | null;
  /** Every run the reads caught on this engine: the tray's median and range. */
  rates: RateBook;
  /** Null until the engine has been read at least once while running. */
  serving: MlxServing | null;
  failedError: string | null;
}

export interface MlxEngineMonitorDeps {
  readStatus(baseUrl: string): Promise<MlxLiveStatusResult>;
  readServing(): Promise<MlxServingRead>;
  /** `http://127.0.0.1:<mlx_engine.port>` from goose's config, or null when it names none. */
  configBaseUrl(): string | null;
  /** Rank 0's base while the distributed run owns this Mac and is up (`distributedLiveBase`). */
  distributedBaseUrl(): string | null;
  /**
   * The route to a linked Mac's engine while one is published (a remote single serves this Mac's
   * chat): its state and goosed's relay to it; null = no route, chat stays on this Mac.
   */
  remoteRoute(): { state: string; baseUrl: string | null } | null;
  swarmRuns(): string[];
  onSnapshot(snapshot: MlxEngineSnapshot): void;
  schedule(fn: () => void, ms: number): () => void;
  intervalMs: number;
}

/** The IPC channel main pushes every snapshot on, the moment it lands. */
export const MLX_ENGINE_SNAPSHOT_CHANNEL = 'mlx-engine-snapshot';

const SNAPSHOT_ENGINES = new Set(['single', 'distributed', 'remote']);
const SNAPSHOT_MODES = new Set(['unknown', 'off', 'mounting', 'running', 'failed', 'reconnecting']);

/** A pushed payload is a snapshot only if its engine and mode are ones main produces. */
export function isMlxEngineSnapshot(value: unknown): value is MlxEngineSnapshot {
  if (value == null || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v.engine === 'string' &&
    SNAPSHOT_ENGINES.has(v.engine) &&
    typeof v.mode === 'string' &&
    SNAPSHOT_MODES.has(v.mode) &&
    typeof v.rates === 'object' &&
    v.rates != null
  );
}

export const INITIAL_SNAPSHOT: MlxEngineSnapshot = {
  engine: 'single',
  mode: 'unknown',
  modelId: null,
  baseUrl: null,
  stats: null,
  statusDetail: null,
  rates: EMPTY_BOOK,
  serving: null,
  failedError: null,
};

/**
 * The `mlx_engine` block of goose's config.yaml, as far as main needs it: the loopback base URL of
 * the configured port and the model goose would mount. Each is null when the block names none —
 * main never supplies a default port of its own (goose owns that default, and a guessed port would
 * read someone else's server as this engine).
 */
export function mlxEngineConfigFromYaml(text: string): {
  baseUrl: string | null;
  modelId: string | null;
} {
  let parsed: unknown;
  try {
    parsed = yaml.parse(text);
  } catch {
    return { baseUrl: null, modelId: null };
  }
  const block = (parsed as { mlx_engine?: { port?: unknown; model_id?: unknown } } | null)
    ?.mlx_engine;
  const port = block?.port;
  const modelId = block?.model_id;
  return {
    baseUrl:
      typeof port === 'number' && Number.isInteger(port) && port > 0
        ? `http://127.0.0.1:${port}`
        : null,
    modelId: typeof modelId === 'string' && modelId ? modelId : null,
  };
}

export class MlxEngineMonitor {
  private report: MlxEngineReport | null = null;
  private snapshot: MlxEngineSnapshot = INITIAL_SNAPSHOT;
  private cancelNext: (() => void) | null = null;
  private inFlight: Promise<void> | null = null;

  constructor(private readonly deps: MlxEngineMonitorDeps) {}

  current(): MlxEngineSnapshot {
    return this.snapshot;
  }

  /** A renderer read goose's engine status: remember it and read the engine now. */
  reportFromRenderer(report: MlxEngineReport): void {
    this.report = report;
    this.wake();
  }

  /** Read now unless a read is already running; a scheduled read is replaced by this one. */
  wake(): void {
    if (this.inFlight) return;
    this.cancelNext?.();
    this.cancelNext = null;
    this.inFlight = this.tick().finally(() => {
      this.inFlight = null;
    });
  }

  /** One read. Exposed for tests; production reads go through `wake`. */
  async tick(): Promise<void> {
    const next = await this.read();
    this.snapshot = next;
    this.deps.onSnapshot(next);
    if (next.mode === 'running' || next.mode === 'mounting' || next.mode === 'reconnecting') {
      this.cancelNext = this.deps.schedule(() => {
        this.cancelNext = null;
        this.wake();
      }, this.deps.intervalMs);
    }
  }

  stop(): void {
    this.cancelNext?.();
    this.cancelNext = null;
  }

  private async read(): Promise<MlxEngineSnapshot> {
    const distributedBase = this.deps.distributedBaseUrl();
    if (distributedBase) return this.readRouted('distributed', distributedBase);
    const route = this.deps.remoteRoute();
    if (route) {
      const base = remoteLiveBase(route);
      return base ? this.readRouted('remote', base) : this.routeUnread(route.state);
    }
    return { ...(await this.readSingle()), engine: 'single' };
  }

  /** A route with nothing to read yet (mounting there, failed there, or no relay handed over). */
  private routeUnread(state: string): MlxEngineSnapshot {
    const mode: MlxEngineMode =
      state === 'mounting' || state === 'failed' || state === 'reconnecting' ? state : 'unknown';
    return {
      ...INITIAL_SNAPSHOT,
      engine: 'remote',
      mode,
      statusDetail: mode === 'unknown' ? `the route is ${state} and names no relay to read` : null,
      rates: this.snapshot.engine === 'remote' ? this.snapshot.rates : EMPTY_BOOK,
    };
  }

  /**
   * The distributed run's rank 0, or a linked Mac's engine through the relay, through the single
   * engine's parser and rates. Its lifecycle is the renderer's report (the tray's distributed and
   * remote branches); this read carries only what it is doing.
   */
  private async readRouted(
    engine: 'distributed' | 'remote',
    baseUrl: string
  ): Promise<MlxEngineSnapshot> {
    const held = this.snapshot.engine === engine ? this.snapshot : null;
    const rates = held?.rates ?? EMPTY_BOOK;
    const result = await this.deps.readStatus(baseUrl);
    if (!result.ok) {
      // A split's rank 0 on this Mac that is slow keeps its last read; a linked Mac's engine read
      // over Link never does — any failed read there (timeout, refused, the relay's 502) is lost
      // contact, named, and the loop keeps reading until the Mac answers again.
      const hold = result.error === 'timeout' && engine === 'distributed';
      return {
        ...INITIAL_SNAPSHOT,
        engine,
        mode:
          engine === 'remote'
            ? 'reconnecting'
            : result.error === 'timeout' && held
              ? held.mode
              : 'unknown',
        baseUrl,
        stats: hold ? (held?.stats ?? null) : null,
        statusDetail: `${result.error}: ${result.detail}`,
        rates,
      };
    }
    const parsed = parseMlxLiveStatus(result.body);
    if (!parsed.ok) {
      return {
        ...INITIAL_SNAPSHOT,
        engine,
        baseUrl,
        statusDetail: parsed.detail,
        rates,
      };
    }
    const stats = parsed.stats;
    return {
      engine,
      mode: 'running',
      modelId: null,
      baseUrl,
      stats,
      statusDetail: null,
      rates: advanceRateBook(rates, stats),
      serving: await this.attribute(stats, engine === 'remote'),
      failedError: null,
    };
  }

  private async attribute(stats: MlxLiveStats, onPeer: boolean): Promise<MlxServing> {
    if (stats.requests.length === 0) {
      return attributeServing([], 0, this.deps.swarmRuns(), null);
    }
    const read = await this.deps.readServing();
    return attributeServing(
      read.ok ? servingRowsForEngine(read.rows, onPeer) : [],
      stats.requests.length,
      this.deps.swarmRuns(),
      read.ok ? null : read.detail
    );
  }

  private async readSingle(): Promise<MlxEngineSnapshot> {
    const report = this.report;
    const rates = this.snapshot.engine === 'single' ? this.snapshot.rates : EMPTY_BOOK;
    const baseUrl = report?.baseUrl ?? this.deps.configBaseUrl();
    const reportedModel = report?.servedModelId ?? report?.modelId ?? null;
    const failedError = report?.state === 'failed' ? (report.lastError ?? null) : null;
    if (!baseUrl) {
      return {
        ...INITIAL_SNAPSHOT,
        mode: report?.state === 'failed' ? 'failed' : 'unknown',
        modelId: reportedModel,
        statusDetail: 'goose names no port for the MLX engine yet',
        rates,
        failedError,
      };
    }
    const result = await this.deps.readStatus(baseUrl);
    if (!result.ok) {
      if (result.error === 'unreachable' || result.error === 'bad-base-url') {
        const mode: MlxEngineMode =
          report?.state === 'mounting' ? 'mounting' : report?.state === 'failed' ? 'failed' : 'off';
        return {
          ...INITIAL_SNAPSHOT,
          mode,
          modelId: mode === 'off' ? null : reportedModel,
          baseUrl,
          statusDetail: `${result.error}: ${result.detail}`,
          rates: mode === 'off' ? EMPTY_BOOK : rates,
          failedError,
        };
      }
      // A timeout is a slow engine, not a gone one: hold what was known and say why it is stale.
      // A non-2xx or a non-JSON body is something on the port that is not answering as Rapid-MLX.
      const prev = this.snapshot.engine === 'single' ? this.snapshot : INITIAL_SNAPSHOT;
      const held = prev.mode === 'running' || prev.mode === 'mounting';
      return {
        ...prev,
        baseUrl,
        mode: result.error === 'timeout' && held ? prev.mode : 'unknown',
        statusDetail: `${result.error}: ${result.detail}`,
      };
    }
    const parsed = parseMlxLiveStatus(result.body);
    if (!parsed.ok) {
      return {
        ...INITIAL_SNAPSHOT,
        mode: 'unknown',
        modelId: reportedModel,
        baseUrl,
        statusDetail: parsed.detail,
        rates,
      };
    }
    const stats = parsed.stats;
    const bodyModel = (result.body as { model?: unknown }).model;
    const engineModel = typeof bodyModel === 'string' && bodyModel ? bodyModel : null;
    const serving = await this.attribute(stats, false);
    return {
      engine: 'single',
      mode: 'running',
      modelId: engineModel ?? reportedModel,
      baseUrl,
      stats,
      statusDetail: null,
      rates: advanceRateBook(rates, stats),
      serving,
      failedError: null,
    };
  }
}
