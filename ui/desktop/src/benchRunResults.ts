/**
 * Per-run results — the pure rules main.ts applies so EVERY finished run keeps its own scored result
 * row (verdict, breakdown, evidence directory) and can be published, not only the latest one.
 *
 * MEASURED 2026-10-03: result.json held only the LAST finished run, so a finished DeepSeek Pro run
 * (0.699, its verdict.json still in its own tree) lost its breakdown and its Publish form the moment the
 * next run finished. Each run's row is now stored under its own key, and a run that predates that store
 * is rebuilt from the verdict in its own tree — never borrowed from a neighbour.
 */

export interface RunIdentity {
  runId: string | null;
  startedAt: string;
  score?: number;
}

/** The run's stable key — the engine's run id, or its start stamp before the id exists. The view keys
 *  sessions the same way (sessionKey), so the renderer and main name a run identically. */
export const benchRunKey = (row: RunIdentity): string => row.runId ?? `start-${row.startedAt}`;

/** A file name for the key: run ids are safe already; a start stamp carries ':' and '.'. */
export const runResultFileName = (key: string): string =>
  `${key.replace(/[^A-Za-z0-9._-]/g, '_')}.json`;

/**
 * Whether a stored result row describes THIS run: the same engine run id, else the same launch stamp.
 * A score the session row recorded must agree too — a reused slot must never lend its verdict.
 */
export function resultDescribesRun(
  result: { runId?: unknown; runMeta?: { startedAt?: unknown }; score?: unknown } | null,
  row: RunIdentity
): boolean {
  if (!result) return false;
  const sameRun =
    (row.runId != null && result.runId === row.runId) ||
    result.runMeta?.startedAt === row.startedAt;
  if (!sameRun) return false;
  return row.score == null || result.score === row.score;
}

/** A verdict read from a run's own tree describes the session only when its score is the one the
 *  session row was stamped with (the close handler stamps both from the same verdict). */
export const treeVerdictDescribesRun = (verdict: { score?: unknown }, row: RunIdentity): boolean =>
  typeof verdict.score === 'number' && (row.score == null || verdict.score === row.score);

export interface PublishedRecord {
  url: string | null;
  title: string;
  score: number;
  publishedAt: string;
}

export type PublishedIndex = Record<string, PublishedRecord>;

/** The published index as read from disk: anything malformed is no record, never a fake one. */
export function parsePublishedIndex(raw: unknown): PublishedIndex {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return {};
  const out: PublishedIndex = {};
  for (const [key, value] of Object.entries(raw as Record<string, unknown>)) {
    const v = value as Partial<PublishedRecord> | null;
    if (
      v &&
      typeof v.title === 'string' &&
      typeof v.score === 'number' &&
      typeof v.publishedAt === 'string' &&
      (typeof v.url === 'string' || v.url === null)
    )
      out[key] = { url: v.url, title: v.title, score: v.score, publishedAt: v.publishedAt };
  }
  return out;
}

/**
 * When a result may publish WITHOUT its graded browser clip — the site's rule, mirrored exactly (website
 * lib/benchmark-recording.ts after review, 2026-10-03; anything else answers 422). The scorer's own rows
 * must say the app never produced anything a browser could record:
 * - Gauntlet (sb-7.1 / sb-7.2): `server_runs` ≤ 0.15 with a no-server detail ("crash at boot…" or
 *   "process survives 5s without binding…") AND `serves_page` exactly 0 with detail exactly
 *   "GET / -> None", no check name twice, and no J/V/P row above 0 (a browser row that earned anything
 *   proves a page existed).
 * - Forge (forge-1.0): v_theme_tokens, v_dark_mode, v_csp_clean and v_console_clean all exactly 0 with
 *   detail exactly "vacuous — precondition unmet: a surface rendered app content". Surfaces that rendered
 *   without a recorded clip are a SCORING problem (retry), never a clip-less post.
 * Returns the absence in words for the run card, or null when a clip is required.
 */
export const FORGE_NO_SURFACE_DETAIL = 'vacuous — precondition unmet: a surface rendered app content';
const FORGE_VISUAL_ROWS = ['v_theme_tokens', 'v_dark_mode', 'v_csp_clean', 'v_console_clean'];
const CLIPLESS_ERAS = new Set(['sb-7.1', 'sb-7.2', 'forge-1.0']);
const NO_SERVER = ['crash at boot', 'process survives 5s without binding'];
const BROWSER_TIERS = new Set(['J', 'V', 'P']);

interface CheckRow {
  check?: unknown;
  tier?: unknown;
  score?: unknown;
  detail?: unknown;
}

export function clipAbsence(scorerVersion: string | undefined, checks: unknown): string | null {
  if (!scorerVersion || !CLIPLESS_ERAS.has(scorerVersion)) return null;
  const rows = (Array.isArray(checks) ? checks : []) as CheckRow[];
  const row = (name: string) => rows.find((r) => r?.check === name);
  const detail = (r: CheckRow | undefined) => (typeof r?.detail === 'string' ? r.detail : '');
  if (scorerVersion === 'forge-1.0') {
    const all = FORGE_VISUAL_ROWS.every((name) => {
      const r = row(name);
      return r != null && r.score === 0 && detail(r) === FORGE_NO_SURFACE_DETAIL;
    });
    return all ? 'no surface rendered app content, so there was nothing to record' : null;
  }
  const names = rows.map((r) => r?.check);
  if (new Set(names).size !== names.length) return null;
  if (rows.some((r) => BROWSER_TIERS.has(r?.tier as string) && typeof r.score === 'number' && r.score > 0))
    return null;
  const serverRuns = row('server_runs');
  const servesPage = row('serves_page');
  if (
    serverRuns != null &&
    typeof serverRuns.score === 'number' &&
    serverRuns.score <= 0.15 &&
    NO_SERVER.some((prefix) => detail(serverRuns).startsWith(prefix)) &&
    servesPage?.score === 0 &&
    detail(servesPage) === 'GET / -> None'
  )
    return `the app never served a page (server_runs: ${detail(serverRuns)}; serves_page: GET / -> None)`;
  return null;
}
