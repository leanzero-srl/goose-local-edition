import type { GooseWindowFacts, Rect } from './utils/engineGlanceRules';

/**
 * Which goose windows can be seen — the fact the desktop glance shows on (Q-226).
 *
 * macOS tracks each window's occlusion: wholly covered by other windows, or on a Space that is not
 * showing. Electron emits the change as the window's 'hide' event and its end as 'show', while
 * `isVisible()` stays true (it only says the window is ordered in). Measured on Electron 41 on the
 * owner's Mac (four displays), a goose-like window (hidden title bar, vibrancy) against a separate
 * app's opaque window:
 *  - another app focused beside it, or on another display → no event, `visibilityState` visible;
 *  - half covered, or covered with a few px of its edge showing → no event, visible;
 *  - covered with ~60 px or more to spare, or another app full screen on its display → 'hide',
 *    `document.visibilityState` hidden; uncovered / full screen left → 'show', visible;
 *  - minimized → 'minimize' + 'hide'; restored → 'restore' + 'show'.
 * A window covered with only a sliver to spare still counts as seen (macOS keeps it visible while
 * any part, rounded corners included, can show); the card then stays away, as it does for a window
 * in plain view.
 */

/** The BrowserWindow surface this reads — Electron-free, so it is tested as data. */
export interface GooseWindowLike {
  on(event: 'hide' | 'show', listener: () => void): unknown;
  isDestroyed(): boolean;
  isVisible(): boolean;
  isMinimized(): boolean;
  getBounds(): Rect;
}

const outOfSight = new WeakSet<GooseWindowLike>();

/** From its creation on: a window's occlusion 'hide' puts it out of sight until its next 'show'. */
export function trackOutOfSight(win: GooseWindowLike): void {
  win.on('hide', () => outOfSight.add(win));
  win.on('show', () => outOfSight.delete(win));
}

export function gooseWindowFacts(
  windows: readonly GooseWindowLike[],
  focused: GooseWindowLike | null
): GooseWindowFacts[] {
  return windows
    .filter((w) => !w.isDestroyed())
    .map((w) => ({
      onScreen: w.isVisible() && !w.isMinimized() && !outOfSight.has(w),
      focused: w === focused,
      bounds: w.getBounds(),
    }));
}
