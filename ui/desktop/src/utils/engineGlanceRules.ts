import type { GlanceCorner, GlancePush } from './engineGlance';

/**
 * WHEN and WHERE the engine glance shows — pure, so every rule is tested as data and main and the
 * renderer decide the same way.
 *
 * What the research said about floating status (macOS Picture in Picture, Spotify's miniplayer,
 * Discord/OBS overlays), and what these rules do about it:
 *  - an always-on-top window people did not ask for is the #1 complaint → the desktop window appears
 *    only while something is LIVE, and by default only while no goose window is on screen (the
 *    sidebar card already shows it wherever goose can be seen — Q-226: "not the active app" put it
 *    over goose's own window on a second display);
 *  - it covers what you are working on → it sits in a screen corner, collapses to a pill, and closing
 *    it dismisses it for the rest of this app session (Q-426: a close that came back with the next
 *    busy spell was "obtrusive") — the person brings it back from the menu-bar icon or Settings › App,
 *    and Settings turns it off for good;
 *  - it flickers between requests → "live" includes a goose session with a turn in flight (the
 *    session-state store's `running`), so the gaps between an agent's model calls, while it runs its
 *    tools, keep it up — a truth the store already has, not a timer;
 *  - it steals focus → main shows it inactive, as a non-activating panel (engineGlanceWindow.ts).
 */

/** Something a person would want to glance at: the engine working, a turn in flight, a question. */
export function glanceLive(push: GlancePush): boolean {
  if (push.sessions.needsYou.length > 0) return true;
  return push.engine.present && (push.engine.busy || push.sessions.running > 0);
}

/** Anything to show at all — an engine present (idle included), or a question waiting. */
export function glanceHasContent(push: GlancePush): boolean {
  return push.engine.present || push.sessions.needsYou.length > 0;
}

export interface DesktopFacts {
  /** A goose window (not the glance) can be seen somewhere on a screen (`gooseOnScreen`). */
  gooseOnScreen: boolean;
  /**
   * The person closed the desktop window in this app session and has not brought it back (Q-426).
   * Held in main only — never a setting: the next launch shows it by the stored mode again.
   */
  dismissed: boolean;
}

export function desktopGlanceVisible(push: GlancePush, facts: DesktopFacts): boolean {
  const mode = push.prefs.desktop;
  if (mode === 'off' || facts.dismissed || !glanceLive(push)) return false;
  return mode === 'busy' || !facts.gooseOnScreen;
}

/**
 * The card at the foot of the sidebar: shown whenever there is something to show (idle included —
 * the quiet grey card) and the person has not hidden it. It takes its own place in the sidebar's
 * column (NavigationPanel.tsx), so the sessions above it end where it begins (Q-216); there is no
 * "room" to measure and nothing floats over the content when there is none (Q-217).
 */
export function dockShown(push: GlancePush): boolean {
  return push.prefs.inApp && glanceHasContent(push);
}

/** The card is hidden by the person and would have something to show: offer it back (Q-218). */
export function dockRestorable(push: GlancePush): boolean {
  return !push.prefs.inApp && glanceHasContent(push);
}

export interface GooseWindowFacts {
  /**
   * Some of it can be seen: shown, not minimized, and — macOS — not wholly covered by other windows
   * nor on a Space that is not showing. main reads the last from the window's occlusion, which
   * Electron reports as the window's 'hide' / 'show' events while `isVisible()` stays true
   * (measured on Electron 41, Q-226: fully covered, another app full screen over it, minimized →
   * 'hide'; uncovered, restored → 'show'; half covered, or visible on one display while another app
   * is focused on another → no event, still on screen).
   */
  onScreen: boolean;
  focused: boolean;
  bounds: Rect;
  /**
   * The share of the window no other app's window covers, read from macOS's window list
   * (engineGlanceCoverage.ts) — null when it was not read (not macOS, goose in front, nothing live
   * to float, or the read failed, which main logs). Q-313: macOS reports no occlusion while a 4–20 px
   * sliver of the window still shows, so `onScreen` alone kept "seen" a window no one could see.
   */
  visibleShare: number | null;
}

// ratio: of a goose window's own area left uncovered, below which — goose behind another app — it
// counts as covered (Q-313). Receipts: Q-226 measured macOS reporting occlusion from ~60 px to spare
// and NOT for 4 or 20 px; Q-313's 2048×1280 window over goose's 2056×1289 left an 8×9 px L = 1.08%
// and no glance showed. A 20 px strip of a 2056-wide window is ~1%, a 60 px one ~3%: 5% sits above
// every measured no-event sliver and far below a half-covered window (50%), which stays "seen".
export const GLANCE_COVERED_SHARE = 0.05;

/**
 * A goose window a person can see: on screen (macOS occlusion) and — while goose is not the app in
 * front — more than a sliver of it left uncovered (Q-313). With goose in front, its focused window is
 * over everything, so a share read while it was behind never counts.
 */
function gooseWindowSeen(w: GooseWindowFacts, gooseInFront: boolean): boolean {
  if (!w.onScreen) return false;
  if (gooseInFront || w.visibleShare == null) return true;
  return w.visibleShare >= GLANCE_COVERED_SHARE;
}

function seenGooseWindows(windows: readonly GooseWindowFacts[]): GooseWindowFacts[] {
  const gooseInFront = windows.some((w) => w.focused);
  return windows.filter((w) => gooseWindowSeen(w, gooseInFront));
}

/**
 * goose can be seen — what the "while goose is in the background" desktop window hangs on (Q-226).
 * macOS: any goose window has more than a sliver on screen (Q-313). Not "goose is the active app":
 * with several displays goose is often in plain view while another app has focus, and the card then
 * floated over goose's own window, duplicating the sidebar card beside it. Not "a goose window is
 * focused" either: goose's own open-folder panel or a menu leaves none focused (Q-217). Elsewhere
 * the occlusion is not reported; a focused goose window is the fact, as before.
 */
export function gooseOnScreen(platform: string, windows: readonly GooseWindowFacts[]): boolean {
  if (platform !== 'darwin') return windows.some((w) => w.focused);
  return seenGooseWindows(windows).length > 0;
}

/**
 * Whether main reads macOS's window list now (Q-313): on macOS, while the desktop window could
 * show (on, not dismissed, something live), with goose behind another app, and while some goose
 * window is still reported on screen — a window macOS already calls covered needs no second read.
 */
export function coverageWanted(
  platform: string,
  push: GlancePush | null,
  dismissed: boolean,
  windows: readonly GooseWindowFacts[]
): boolean {
  if (platform !== 'darwin' || push == null) return false;
  if (push.prefs.desktop === 'off' || dismissed || !glanceLive(push)) return false;
  if (windows.some((w) => w.focused)) return false;
  return windows.some((w) => w.onScreen);
}

/** The share of `target` no rect in `above` covers — exact, over the grid of every rect's edges. */
export function visibleShare(target: Rect, above: readonly Rect[]): number {
  const area = target.width * target.height;
  if (area <= 0) return 0;
  const clipped = above
    .map((r) => ({
      x0: Math.max(r.x, target.x),
      y0: Math.max(r.y, target.y),
      x1: Math.min(r.x + r.width, target.x + target.width),
      y1: Math.min(r.y + r.height, target.y + target.height),
    }))
    .filter((r) => r.x0 < r.x1 && r.y0 < r.y1);
  const edges = (ends: number[]) => [...new Set(ends)].sort((a, b) => a - b);
  const xs = edges([target.x, target.x + target.width, ...clipped.flatMap((r) => [r.x0, r.x1])]);
  const ys = edges([target.y, target.y + target.height, ...clipped.flatMap((r) => [r.y0, r.y1])]);
  let covered = 0;
  for (let i = 0; i + 1 < xs.length; i++) {
    for (let j = 0; j + 1 < ys.length; j++) {
      const cx = (xs[i] + xs[i + 1]) / 2;
      const cy = (ys[j] + ys[j + 1]) / 2;
      if (clipped.some((r) => r.x0 <= cx && cx < r.x1 && r.y0 <= cy && cy < r.y1)) {
        covered += (xs[i + 1] - xs[i]) * (ys[j + 1] - ys[j]);
      }
    }
  }
  return (area - covered) / area;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** The corner nearest the window's centre — where a drag release snaps it. */
export function nearestCorner(bounds: Rect, workArea: Rect): GlanceCorner {
  const cx = bounds.x + bounds.width / 2;
  const cy = bounds.y + bounds.height / 2;
  const left = cx < workArea.x + workArea.width / 2;
  const top = cy < workArea.y + workArea.height / 2;
  return `${top ? 'top' : 'bottom'}-${left ? 'left' : 'right'}` as GlanceCorner;
}

/** The window's bounds snapped into `corner` of the work area, `margin` in from both edges. */
export function cornerBounds(
  corner: GlanceCorner,
  size: { width: number; height: number },
  workArea: Rect,
  margin: number
): Rect {
  const width = Math.min(size.width, workArea.width - 2 * margin);
  const height = Math.min(size.height, workArea.height - 2 * margin);
  const x = corner.endsWith('left')
    ? workArea.x + margin
    : workArea.x + workArea.width - margin - width;
  const y = corner.startsWith('top')
    ? workArea.y + margin
    : workArea.y + workArea.height - margin - height;
  return {
    x: Math.round(x),
    y: Math.round(y),
    width: Math.round(width),
    height: Math.round(height),
  };
}

export interface GlanceDisplay {
  id: number;
  workArea: Rect;
}

function intersects(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}

function distanceTo(p: { x: number; y: number }, r: Rect): number {
  const dx = Math.max(r.x - p.x, 0, p.x - (r.x + r.width));
  const dy = Math.max(r.y - p.y, 0, p.y - (r.y + r.height));
  return Math.hypot(dx, dy);
}

/** The display a point is on (or, past every work area — a menu bar, a Dock — the nearest one). */
function displayAt(displays: readonly GlanceDisplay[], p: { x: number; y: number }): GlanceDisplay {
  return displays.reduce((best, d) =>
    distanceTo(p, d.workArea) < distanceTo(p, best.workArea) ? d : best
  );
}

/**
 * The display the person is working on: the focused goose window's, else the pointer's. With goose
 * out of sight the focused window is another app's, which Electron cannot see — the pointer is
 * where the person is.
 */
export function workingDisplay(
  displays: readonly GlanceDisplay[],
  windows: readonly GooseWindowFacts[],
  cursor: { x: number; y: number }
): GlanceDisplay {
  const focused = windows.find((w) => w.focused && w.onScreen);
  if (focused) {
    const b = focused.bounds;
    return displayAt(displays, { x: b.x + b.width / 2, y: b.y + b.height / 2 });
  }
  return displayAt(displays, cursor);
}

const CORNERS: readonly GlanceCorner[] = ['bottom-right', 'top-right', 'bottom-left', 'top-left'];

export interface GlancePlacement {
  displayId: number;
  corner: GlanceCorner;
  bounds: Rect;
}

/**
 * Where the desktop window shows (Q-226): on the display the person is working on, in the corner
 * they last dropped it in (the one remembered place, applied to whichever display it shows on) —
 * and NEVER over a goose window that can be seen. A corner that would cover one gives way to the
 * next corner, then to the other displays (the remembered one first); with every spot over goose,
 * null: it does not show. With goose out of sight (the default mode's only case) the first choice
 * always stands.
 */
export function placeGlance(input: {
  displays: readonly GlanceDisplay[];
  working: GlanceDisplay;
  remembered: { displayId: number; corner: GlanceCorner } | null;
  defaultCorner: GlanceCorner;
  windows: readonly GooseWindowFacts[];
  size: { width: number; height: number };
  margin: number;
}): GlancePlacement | null {
  const first = input.remembered?.corner ?? input.defaultCorner;
  const corners = [first, ...CORNERS.filter((c) => c !== first)];
  const remembered = input.displays.find((d) => d.id === input.remembered?.displayId);
  const others = input.displays.filter((d) => d.id !== input.working.id && d !== remembered);
  const order = [
    input.working,
    ...(remembered && remembered.id !== input.working.id ? [remembered] : []),
    ...others,
  ];
  const seen = seenGooseWindows(input.windows).map((w) => w.bounds);
  for (const display of order) {
    for (const corner of corners) {
      const bounds = cornerBounds(corner, input.size, display.workArea, input.margin);
      if (!seen.some((w) => intersects(w, bounds))) {
        return { displayId: display.id, corner, bounds };
      }
    }
  }
  return null;
}

/** Still clear of every goose window that can be seen — else it must move (or go). */
export function clearOfGoose(bounds: Rect, windows: readonly GooseWindowFacts[]): boolean {
  return !seenGooseWindows(windows).some((w) => intersects(w.bounds, bounds));
}
