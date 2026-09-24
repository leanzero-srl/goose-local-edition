import * as yaml from 'yaml';
import {
  NO_RATES,
  advanceLastRates,
  parseMlxLiveStatus,
  type LastRates,
  type MlxLiveStats,
} from '../components/leanzero-swarm/mlxLiveStats';
import type { MlxLiveStatusResult } from './mlxLiveStatus';
import { attributeServing, type MlxServing, type MlxServingRead } from './mlxServing';

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
 */

export type MlxEngineMode = 'unknown' | 'off' | 'mounting' | 'running' | 'failed';

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
   * Which engine this read is of: the single engine, or the distributed run's rank 0 while that run
   * owns this Mac and is up (its `/v1/status` answers in the single engine's shape).
   */
  engine: 'single' | 'distributed';
  mode: MlxEngineMode;
  /** The id the engine serves (its own `/v1/status` model, else goose's served id, else the HF id). */
  modelId: string | null;
  baseUrl: string | null;
  stats: MlxLiveStats | null;
  /** Why `stats` is absent or stale, verbatim. */
  statusDetail: string | null;
  last: LastRates;
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
  swarmRuns(): string[];
  onSnapshot(snapshot: MlxEngineSnapshot): void;
  schedule(fn: () => void, ms: number): () => void;
  intervalMs: number;
}

export const INITIAL_SNAPSHOT: MlxEngineSnapshot = {
  engine: 'single',
  mode: 'unknown',
  modelId: null,
  baseUrl: null,
  stats: null,
  statusDetail: null,
  last: NO_RATES,
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
    if (next.mode === 'running' || next.mode === 'mounting') {
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
    if (distributedBase) return this.readDistributed(distributedBase);
    return { ...(await this.readSingle()), engine: 'single' };
  }

  /**
   * The distributed run's rank 0, through the single engine's parser and rates. Its lifecycle is
   * the renderer's report (the tray's distributed branch); this read carries only what it is doing.
   */
  private async readDistributed(baseUrl: string): Promise<MlxEngineSnapshot> {
    const held = this.snapshot.engine === 'distributed' ? this.snapshot : null;
    const last = held?.last ?? NO_RATES;
    const result = await this.deps.readStatus(baseUrl);
    if (!result.ok) {
      return {
        ...INITIAL_SNAPSHOT,
        engine: 'distributed',
        mode: result.error === 'timeout' && held ? held.mode : 'unknown',
        baseUrl,
        stats: result.error === 'timeout' ? (held?.stats ?? null) : null,
        statusDetail: `${result.error}: ${result.detail}`,
        last,
      };
    }
    const parsed = parseMlxLiveStatus(result.body);
    if (!parsed.ok) {
      return {
        ...INITIAL_SNAPSHOT,
        engine: 'distributed',
        baseUrl,
        statusDetail: parsed.detail,
        last,
      };
    }
    const stats = parsed.stats;
    return {
      engine: 'distributed',
      mode: 'running',
      modelId: null,
      baseUrl,
      stats,
      statusDetail: null,
      last: advanceLastRates(last, stats),
      serving: await this.attribute(stats),
      failedError: null,
    };
  }

  private async attribute(stats: MlxLiveStats): Promise<MlxServing> {
    if (stats.requests.length === 0) {
      return attributeServing([], 0, this.deps.swarmRuns(), null);
    }
    const read = await this.deps.readServing();
    return attributeServing(
      read.ok ? read.rows : [],
      stats.requests.length,
      this.deps.swarmRuns(),
      read.ok ? null : read.detail
    );
  }

  private async readSingle(): Promise<MlxEngineSnapshot> {
    const report = this.report;
    const last = this.snapshot.engine === 'single' ? this.snapshot.last : NO_RATES;
    const baseUrl = report?.baseUrl ?? this.deps.configBaseUrl();
    const reportedModel = report?.servedModelId ?? report?.modelId ?? null;
    const failedError = report?.state === 'failed' ? (report.lastError ?? null) : null;
    if (!baseUrl) {
      return {
        ...INITIAL_SNAPSHOT,
        mode: report?.state === 'failed' ? 'failed' : 'unknown',
        modelId: reportedModel,
        statusDetail: 'goose names no port for the MLX engine yet',
        last,
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
          last: mode === 'off' ? NO_RATES : last,
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
        last,
      };
    }
    const stats = parsed.stats;
    const bodyModel = (result.body as { model?: unknown }).model;
    const engineModel = typeof bodyModel === 'string' && bodyModel ? bodyModel : null;
    const serving = await this.attribute(stats);
    return {
      engine: 'single',
      mode: 'running',
      modelId: engineModel ?? reportedModel,
      baseUrl,
      stats,
      statusDetail: null,
      last: advanceLastRates(last, stats),
      serving,
      failedError: null,
    };
  }
}
