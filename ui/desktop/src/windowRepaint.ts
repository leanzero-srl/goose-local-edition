/**
 * A goose window whose page stopped being drawn part of the way down is drawn again in full.
 *
 * Seen on the Mac Studio after long unattended benchmark runs (2026-10-19): the sidebar was drawn to the bottom, the
 * main pane only to about half its height, and the rest showed the window's vibrancy grey. Nothing in the layout sets a
 * height (the routes fill the window with flex), so the page was laid out in full and the compositor never drew the rest.
 * The same class of failure in another Electron app on Apple Silicon (anthropics/claude-code#26080) clears when the window
 * is repainted, e.g. by switching away and back. So goose repaints a window whenever it comes back into view, gains focus,
 * or the display or the Mac's session changes, and once a minute while it can be seen, which bounds a stuck frame to a
 * minute even when no event comes. The cause is not proven: main.ts logs every GPU or renderer process that goes away, so
 * the next occurrence names whether the GPU process died.
 */

/** The BrowserWindow surface this uses — Electron-free, so it is tested as data. */
export interface RepaintableWindow {
  on(event: 'focus' | 'show' | 'restore' | 'closed', listener: () => void): unknown;
  isDestroyed(): boolean;
  isVisible(): boolean;
  isMinimized(): boolean;
  webContents: { invalidate(): void };
}

export const REPAINT_INTERVAL_MS = 60_000;

/** Repaints the whole page of a window that can be seen; true when it did. */
export function repaintWindow(win: RepaintableWindow): boolean {
  if (win.isDestroyed() || !win.isVisible() || win.isMinimized()) return false;
  win.webContents.invalidate();
  return true;
}

type Every = (fn: () => void, ms: number) => { unref?: () => unknown };
type Stop = (timer: ReturnType<Every>) => void;

/** From its creation on: repaint on focus, show (also the end of an occlusion) and restore, and once a minute. */
export function keepRepainted(
  win: RepaintableWindow,
  every: Every = setInterval as unknown as Every,
  stop: Stop = clearInterval as unknown as Stop
): void {
  const repaint = () => {
    repaintWindow(win);
  };
  win.on('focus', repaint);
  win.on('show', repaint);
  win.on('restore', repaint);
  const timer = every(repaint, REPAINT_INTERVAL_MS);
  timer.unref?.();
  win.on('closed', () => stop(timer));
}
