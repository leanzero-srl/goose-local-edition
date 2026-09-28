import { visibleShare, type Rect } from './utils/engineGlanceRules';

/**
 * How much of each goose window another app's window leaves uncovered — the fact macOS's occlusion
 * does not give (Q-313).
 *
 * macOS reports a window occluded only once ~60 px or more of it is covered-with-room-to-spare
 * (Q-226's probe); with a 4–20 px sliver left it reports nothing, and the critic's 2048×1280 window
 * over goose's 2056×1289 left an 8×9 px L while no glance showed. The window server's list
 * (CGWindowListCopyWindowInfo, front to back) carries every on-screen window's bounds and owner
 * without any permission — only titles need Screen Recording, and none are read. JXA reaches it
 * through `osascript` (measured ~70 ms on this Mac), so no native module ships for it.
 *
 * Only normal-level windows (layer 0 — the level goose's own windows are on) of OTHER processes
 * count as covering: the menu bar, the Dock and overlay levels are not "another app over goose",
 * and goose's own glance panel must never count as covering goose.
 */

export interface ListedWindow {
  /** kCGWindowNumber — the id Electron's `getMediaSourceId()` carries ("window:<id>:0"). */
  number: number;
  pid: number;
  bounds: Rect;
}

// Front to back, normal-level, visible windows: [{number, pid, bounds}] as JSON on stdout.
export const WINDOW_LIST_JXA = [
  'ObjC.import("CoreGraphics");',
  'const listed = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo(' +
    '$.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements, $.kCGNullWindowID)));',
  'JSON.stringify(listed.filter((w) => w.kCGWindowLayer === 0 && w.kCGWindowAlpha > 0).map((w) => ({' +
    ' number: w.kCGWindowNumber, pid: w.kCGWindowOwnerPID, bounds: { x: w.kCGWindowBounds.X,' +
    ' y: w.kCGWindowBounds.Y, width: w.kCGWindowBounds.Width, height: w.kCGWindowBounds.Height } })));',
].join('\n');

const isFiniteNumber = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);

/** The script's stdout, checked field by field — a shape it does not recognise is an error, never []. */
export function parseWindowList(stdout: string): ListedWindow[] {
  const parsed: unknown = JSON.parse(stdout);
  if (!Array.isArray(parsed)) throw new Error('window list: not an array');
  return parsed.map((entry, i) => {
    const w = entry as { number?: unknown; pid?: unknown; bounds?: Record<string, unknown> };
    const b = w.bounds ?? {};
    if (
      !isFiniteNumber(w.number) ||
      !isFiniteNumber(w.pid) ||
      !isFiniteNumber(b.x) ||
      !isFiniteNumber(b.y) ||
      !isFiniteNumber(b.width) ||
      !isFiniteNumber(b.height)
    ) {
      throw new Error(`window list: entry ${i} is not {number, pid, bounds}`);
    }
    return {
      number: w.number,
      pid: w.pid,
      bounds: { x: b.x, y: b.y, width: b.width, height: b.height },
    };
  });
}

/**
 * The share of window `number` that the windows in front of it — other processes' only — leave
 * uncovered. null: the window is not in the on-screen list (minimized, or on a Space not showing;
 * macOS's own occlusion says those).
 */
export function visibleShareInList(
  list: readonly ListedWindow[],
  ownPid: number,
  number: number
): number | null {
  const at = list.findIndex((w) => w.number === number);
  if (at < 0) return null;
  const above = list
    .slice(0, at)
    .filter((w) => w.pid !== ownPid)
    .map((w) => w.bounds);
  return visibleShare(list[at].bounds, above);
}

/** The window number inside Electron's `getMediaSourceId()` ("window:<number>:0"). */
export function windowNumberOf(mediaSourceId: string): number | null {
  const match = /^window:(\d+):/.exec(mediaSourceId);
  return match ? Number(match[1]) : null;
}

export interface CoverageDeps {
  ownPid: number;
  /** Runs WINDOW_LIST_JXA; resolves with its stdout. */
  readWindowList(): Promise<string>;
  /** A read landed that changes what some goose window's share is: re-decide the glance. */
  onChanged(): void;
  /** A read failed — said once per distinct reason, and the shares are unknown until one succeeds. */
  warn(message: string): void;
}

/**
 * One read in flight at a time; a request while one runs asks for exactly one more after it, so the
 * list a decision uses is never older than the last request. Nothing reads on a clock: main asks on
 * the facts that already re-decide the glance (engine snapshots, window focus and occlusion events).
 */
export class GlanceCoverage {
  private list: ListedWindow[] | null = null;
  private reading = false;
  private again = false;
  private lastFailure: string | null = null;
  /** Bumped by forget(): a read that began before it lands on nothing. */
  private generation = 0;

  constructor(private readonly deps: CoverageDeps) {}

  /** Read the window list now (or right after the read in flight). */
  measure(): void {
    if (this.reading) {
      this.again = true;
      return;
    }
    this.reading = true;
    const generation = this.generation;
    void this.deps
      .readWindowList()
      .then((stdout) => {
        const next = parseWindowList(stdout);
        if (generation !== this.generation) return false;
        // Only goose's own windows' shares re-decide anything: another app's window moving elsewhere
        // is not a change worth a refresh (which would ask for yet another read).
        const changed = this.sharesKey(next) !== this.sharesKey(this.list);
        this.list = next;
        this.lastFailure = null;
        return changed;
      })
      .catch((error: unknown) => {
        const reason = error instanceof Error ? error.message : String(error);
        if (reason !== this.lastFailure)
          this.deps.warn(`[engine glance] window list read failed: ${reason}`);
        this.lastFailure = reason;
        if (generation !== this.generation) return false;
        const changed = this.list != null;
        this.list = null;
        return changed;
      })
      .then((changed) => {
        this.reading = false;
        if (this.again) {
          this.again = false;
          this.measure();
        }
        if (changed) this.deps.onChanged();
      });
  }

  /** Goose came to the front, or nothing could float: a list read while it was behind is dropped. */
  forget(): void {
    this.generation++;
    this.again = false;
    this.list = null;
  }

  private sharesKey(list: readonly ListedWindow[] | null): string {
    if (list == null) return 'unread';
    const own = list.filter((w) => w.pid === this.deps.ownPid);
    return JSON.stringify(
      own.map((w) => [w.number, visibleShareInList(list, this.deps.ownPid, w.number)])
    );
  }

  /** The share of the window with this media source id left uncovered; null when not read. */
  visibleShareOf(mediaSourceId: string): number | null {
    if (this.list == null) return null;
    const number = windowNumberOf(mediaSourceId);
    if (number == null) return null;
    return visibleShareInList(this.list, this.deps.ownPid, number);
  }
}
