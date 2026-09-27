import type { GlanceCorner, GlancePush } from './engineGlance';

/**
 * WHEN and WHERE the engine glance shows — pure, so every rule is tested as data and main and the
 * renderer decide the same way.
 *
 * What the research said about floating status (macOS Picture in Picture, Spotify's miniplayer,
 * Discord/OBS overlays), and what these rules do about it:
 *  - an always-on-top window people did not ask for is the #1 complaint → the desktop window appears
 *    only while something is LIVE, and by default only while goose is in the background (the sidebar
 *    card already shows it when goose is in front);
 *  - it covers what you are working on → it sits in a screen corner, collapses to a pill, and closing
 *    it snoozes it for the rest of this busy spell (the next spell brings it back; Settings turns it
 *    off for good);
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
  /** A goose window (not the glance) is focused: goose is the app in front. */
  appInFront: boolean;
  /** The person closed the desktop window during this live spell. */
  snoozed: boolean;
}

export function desktopGlanceVisible(push: GlancePush, facts: DesktopFacts): boolean {
  const mode = push.prefs.desktop;
  if (mode === 'off' || facts.snoozed || !glanceLive(push)) return false;
  return mode === 'busy' || !facts.appInFront;
}

/** A close snoozes the window until the live spell ends; the next spell shows it again. */
export function snoozeAfter(snoozed: boolean, push: GlancePush): boolean {
  return snoozed && glanceLive(push);
}

export type InAppPlacement = 'hidden' | 'dock' | 'float';

export interface InAppFacts {
  navExpanded: boolean;
  /** The sidebar's trees leave room below them for the docked card. */
  dockRoom: boolean;
}

/**
 * The sidebar's empty space when it has room (idle included — a quiet grey line); a floating card
 * over the window only while something is LIVE (an idle engine never covers content).
 */
export function inAppPlacement(push: GlancePush, facts: InAppFacts): InAppPlacement {
  if (!push.prefs.inApp || !glanceHasContent(push)) return 'hidden';
  if (facts.navExpanded && facts.dockRoom) return 'dock';
  return glanceLive(push) ? 'float' : 'hidden';
}

/**
 * Whether the docked card fits: the trees' own height plus the card's must fit the column. The card
 * already docked frees its own height back when it is measured, so a card that fits stays (and one
 * that does not, floats) without flapping at the boundary.
 */
export function dockFits(column: number, trees: number, card: number): boolean {
  return card > 0 && trees + card <= column;
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
