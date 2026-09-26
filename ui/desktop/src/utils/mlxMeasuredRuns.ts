import type { MlxSpeedFigureDto } from '@aaif/goose-sdk';

/**
 * goose's measured runs, as every desktop surface reads them. goose keeps every measured run per
 * model and per way (this Mac, a linked Mac, the split) in its measurement store under its data dir,
 * and ONE reader there (goose-sidecar `placement::runs`) decides which runs count and makes their
 * figure. The Engine tile and the Run it cards read it inside goose's plan; MAIN — the menu-bar tray
 * and the chat's reading estimate — reads it from goosed's `GET /mlx-engine/measured-runs`, keyed by
 * the very function goose records a finished turn under. Nothing here counts runs again, and nothing
 * is kept in memory, so a relaunch loses none of them and no two surfaces can disagree (Q-129).
 */

export type SpeedFigure = MlxSpeedFigureDto;

/**
 * A MEASURED figure as every surface says it: how many runs, their median, and their middle half only
 * when the runs differ — one run is "1 run · 29.6 tok/s", never a "29.6–29.6" range (Q-123). null for
 * an estimate or a figure with no run behind it: an estimate is never called a measurement.
 */
export interface MeasuredFigure {
  runs: number;
  median: number;
  /** The runs' middle half (all of them up to four); absent for one run, or runs that all agree. */
  spread?: { low: number; high: number };
  lastMeasuredMs: number | null;
}

export function measuredFigure(figure: SpeedFigure | null | undefined): MeasuredFigure | null {
  if (!figure || !figure.measured || figure.runs < 1) return null;
  const { value, low, high } = figure.estimate;
  return {
    runs: figure.runs,
    median: value,
    spread: figure.runs > 1 && high > low ? { low, high } : undefined,
    lastMeasuredMs: figure.lastMeasuredMs ?? null,
  };
}

/** The way goose's MLX chat runs now, keyed as the plan keys its candidates. */
export interface MeasuredWay {
  placementId: string;
  placement: { kind: 'single' | 'tensor' | 'pipeline'; nodes: string[]; link?: string | null };
  modelId: string;
  nodeNames: string[];
}

/** One goosed's `GET /mlx-engine/measured-runs` answer. */
export interface MeasuredRunsAnswer {
  way: MeasuredWay | null;
  wayError: string | null;
  recorded: number;
  writing: SpeedFigure | null;
  writingBasis: string | null;
  /** Reading at the chat goal's prompt size — the figure the Run it card's chat plan shows. */
  reading: SpeedFigure | null;
  readingByBucket: { bucket: number; figure: SpeedFigure }[];
  storeErrors: string[];
}

/** What a surface has of the runs of the engine it shows. */
export type MlxMeasuredRead =
  | { kind: 'pending' }
  | { kind: 'unread'; detail: string }
  | { kind: 'read'; answer: MeasuredRunsAnswer };

export const MEASURED_PENDING: MlxMeasuredRead = { kind: 'pending' };

type FetchLike = (url: string, init: RequestInit) => Promise<Response>;

const isObj = (v: unknown): v is Record<string, unknown> => v != null && typeof v === 'object';
const isNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);

function isFigure(v: unknown): v is SpeedFigure {
  if (!isObj(v) || !isObj(v.estimate)) return false;
  const e = v.estimate;
  return (
    isNum(e.value) &&
    isNum(e.low) &&
    isNum(e.high) &&
    typeof v.measured === 'boolean' &&
    isNum(v.runs)
  );
}

const figureOrNull = (v: unknown) => v === null || v === undefined || isFigure(v);

function isWay(v: unknown): v is MeasuredWay {
  if (!isObj(v) || !isObj(v.placement)) return false;
  const p = v.placement;
  return (
    typeof v.placementId === 'string' &&
    typeof v.modelId === 'string' &&
    (p.kind === 'single' || p.kind === 'tensor' || p.kind === 'pipeline') &&
    Array.isArray(p.nodes) &&
    p.nodes.every((n) => typeof n === 'string')
  );
}

/** A body is an answer only if every field is what goosed sends; otherwise the reason. */
export function parseMeasuredRuns(body: unknown): MeasuredRunsAnswer | string {
  if (!isObj(body)) return 'goose backend answered without a measured-runs body';
  if (!(body.way === null || isWay(body.way))) return 'the measured-runs way is malformed';
  if (!figureOrNull(body.writing) || !figureOrNull(body.reading)) {
    return 'a measured-runs figure is malformed';
  }
  const buckets = body.readingByBucket;
  if (
    !Array.isArray(buckets) ||
    !buckets.every((b) => isObj(b) && isNum(b.bucket) && isFigure(b.figure))
  ) {
    return 'the measured-runs reading buckets are malformed';
  }
  return {
    way: (body.way as MeasuredWay | null) ?? null,
    wayError: typeof body.wayError === 'string' ? body.wayError : null,
    recorded: isNum(body.recorded) ? body.recorded : 0,
    writing: (body.writing as SpeedFigure | null | undefined) ?? null,
    writingBasis: typeof body.writingBasis === 'string' ? body.writingBasis : null,
    reading: (body.reading as SpeedFigure | null | undefined) ?? null,
    readingByBucket: buckets as { bucket: number; figure: SpeedFigure }[],
    storeErrors: Array.isArray(body.storeErrors)
      ? body.storeErrors.filter((e): e is string => typeof e === 'string')
      : [],
  };
}

export type MeasuredRunsFetch =
  | { ok: true; answer: MeasuredRunsAnswer }
  | { ok: false; detail: string };

/** One backend's `GET /mlx-engine/measured-runs`, under its own secret; every failure is named. */
export async function fetchMlxMeasuredRuns(
  httpBase: string,
  secretKey: string,
  fetchImpl: FetchLike,
  timeoutMs: number
): Promise<MeasuredRunsFetch> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetchImpl(`${httpBase}/mlx-engine/measured-runs`, {
      method: 'GET',
      headers: { 'X-Secret-Key': secretKey },
      signal: controller.signal,
    });
    if (res.status === 404) {
      return { ok: false, detail: 'this goose backend predates /mlx-engine/measured-runs' };
    }
    if (!res.ok) {
      const text = await res.text().catch(() => '');
      return {
        ok: false,
        detail: `goose backend returned ${res.status}${text ? `: ${text}` : ''}`,
      };
    }
    const parsed = parseMeasuredRuns(await res.json());
    return typeof parsed === 'string'
      ? { ok: false, detail: parsed }
      : { ok: true, answer: parsed };
  } catch (err) {
    if (err instanceof Error && err.name === 'AbortError') {
      return { ok: false, detail: `goose backend did not answer within ${timeoutMs} ms` };
    }
    return { ok: false, detail: err instanceof Error ? err.message : String(err) };
  } finally {
    clearTimeout(timer);
  }
}

/** Which engine MAIN's loop reads: the single engine here, a linked Mac's through the relay, or the split. */
export type MeasuredEngine = 'single' | 'remote' | 'distributed';

function wayIsEngine(way: MeasuredWay, engine: MeasuredEngine): boolean {
  const { kind, nodes } = way.placement;
  switch (engine) {
    case 'single':
      return kind === 'single' && nodes[0] === 'local';
    case 'remote':
      return kind === 'single' && (nodes[0] ?? '').startsWith('link:');
    case 'distributed':
      return kind === 'tensor' || kind === 'pipeline';
  }
}

/**
 * The runs of the engine MAIN reads, from every goose backend's answer: the first whose way IS that
 * engine. A backend whose chat runs another way, or none, is said — never read as this engine's runs.
 */
export function pickMeasured(
  answers: readonly MeasuredRunsFetch[],
  engine: MeasuredEngine
): MlxMeasuredRead {
  if (answers.length === 0) return { kind: 'unread', detail: 'no goose backend is running' };
  for (const a of answers) {
    if (a.ok && a.answer.way && wayIsEngine(a.answer.way, engine)) {
      return { kind: 'read', answer: a.answer };
    }
  }
  const first = answers[0];
  if (!first.ok) return { kind: 'unread', detail: first.detail };
  if (!first.answer.way) {
    return { kind: 'unread', detail: first.answer.wayError ?? 'goose names no way its chat runs' };
  }
  return {
    kind: 'unread',
    detail: `goose records this Mac's chat on ${first.answer.way.placementId}, not on the engine read here`,
  };
}

/** The store's bucket for a prompt: the power of two at or above it (store.rs `context_bucket`). */
export function contextBucketOf(tokens: number): number {
  let bucket = 1;
  while (bucket < Math.max(1, tokens)) bucket *= 2;
  return bucket;
}

/** The measured reading rate for a prompt of `promptTokens` on this way — only runs of its bucket. */
export function readingForPrompt(
  read: MlxMeasuredRead,
  promptTokens: number | null
): number | null {
  if (read.kind !== 'read' || promptTokens == null || promptTokens <= 0) return null;
  const bucket = contextBucketOf(promptTokens);
  const hit = read.answer.readingByBucket.find((b) => b.bucket === bucket);
  return measuredFigure(hit?.figure)?.median ?? null;
}

export function isMlxMeasuredRead(value: unknown): value is MlxMeasuredRead {
  if (!isObj(value)) return false;
  if (value.kind === 'pending') return true;
  if (value.kind === 'unread') return typeof value.detail === 'string';
  return value.kind === 'read' && typeof parseMeasuredRuns(value.answer) !== 'string';
}
