import {
  ENGINE_GLANCE_CHANNEL,
  type GlanceCorner,
  type GlancePrefs,
  type GlancePush,
} from './utils/engineGlance';
import {
  cornerBounds,
  desktopGlanceVisible,
  nearestCorner,
  snoozeAfter,
  type Rect,
} from './utils/engineGlanceRules';

/**
 * The engine glance as a SYSTEM-LEVEL floating mini window: shown while something is live and goose
 * is in the background (or whenever something is live, by choice), in a corner of a display, over
 * every app and every full-screen Space, never taking focus.
 *
 * The controller is Electron-free — main hands it a `GlanceWindowPort` (engineGlanceWindow.ts) — so
 * every transition (show inactive, snooze, snap, remember the display) is tested as data.
 */

/** What the glance's renderer asks main to do. */
export type GlancePipAction =
  | { type: 'open-engine' }
  | { type: 'open-session'; sessionId: string }
  | { type: 'collapse'; collapsed: boolean }
  | { type: 'close' }
  | { type: 'drag-start'; screenX: number; screenY: number }
  | { type: 'drag-move'; screenX: number; screenY: number }
  | { type: 'drag-end' }
  | { type: 'size'; width: number; height: number };

export const GLANCE_PIP_ACTION_CHANNEL = 'engine-glance-pip';

export function isGlancePipAction(value: unknown): value is GlancePipAction {
  if (value == null || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  const finite = (n: unknown) => typeof n === 'number' && Number.isFinite(n);
  switch (v.type) {
    case 'open-engine':
    case 'close':
    case 'drag-end':
      return true;
    case 'open-session':
      return typeof v.sessionId === 'string' && v.sessionId.length > 0;
    case 'collapse':
      return typeof v.collapsed === 'boolean';
    case 'drag-start':
    case 'drag-move':
      return finite(v.screenX) && finite(v.screenY);
    case 'size':
      return (
        finite(v.width) && finite(v.height) && (v.width as number) > 0 && (v.height as number) > 0
      );
    default:
      return false;
  }
}

export interface GlanceDisplay {
  id: number;
  workArea: Rect;
}

/** The Electron side, as the controller needs it. */
export interface GlanceWindowPort {
  /** Creates the window (hidden) the first time; later calls are no-ops. */
  ensure(): void;
  destroy(): void;
  exists(): boolean;
  /** Shown without activating goose or taking focus. */
  showInactive(): void;
  hide(): void;
  isVisible(): boolean;
  getBounds(): Rect;
  setBounds(bounds: Rect, animate: boolean): void;
  send(channel: string, payload: unknown): void;
  /** The displays now, and the one a rect sits on most. */
  displays(): GlanceDisplay[];
  displayMatching(rect: Rect): GlanceDisplay;
  primaryDisplay(): GlanceDisplay;
}

export interface GlanceDesktopDeps {
  port: GlanceWindowPort;
  appInFront(): boolean;
  savePrefs(next: GlancePrefs): void;
  openEngine(): void;
  openSession(sessionId: string): void;
}

/** px from the work area's edges — the gap macOS leaves around its own Picture in Picture. */
export const GLANCE_MARGIN = 16;

export class EngineGlanceDesktop {
  private push: GlancePush | null = null;
  private snoozed = false;
  private size: { width: number; height: number } | null = null;
  private drag: { pointerX: number; pointerY: number; bounds: Rect; moved: boolean } | null = null;

  constructor(private readonly deps: GlanceDesktopDeps) {}

  /** The latest glance: pushed to the window, then shown or hidden by the rules. */
  update(push: GlancePush): void {
    this.push = push;
    this.snoozed = snoozeAfter(this.snoozed, push);
    if (this.deps.port.exists()) this.deps.port.send(ENGINE_GLANCE_CHANNEL, push);
    this.refresh();
  }

  /** Re-decide after a fact the glance does not carry changed (goose came to the front, or left). */
  refresh(): void {
    const { port } = this.deps;
    const push = this.push;
    const visible =
      push != null &&
      desktopGlanceVisible(push, { appInFront: this.deps.appInFront(), snoozed: this.snoozed });
    if (!visible) {
      if (port.exists() && port.isVisible()) port.hide();
      // Turned off: nothing floats and nothing is kept alive for it.
      if (push?.prefs.desktop === 'off' && port.exists()) port.destroy();
      return;
    }
    if (!port.exists()) {
      port.ensure();
      port.send(ENGINE_GLANCE_CHANNEL, push);
    }
    // First show waits for the renderer's measured size: a window shown at a guessed size jumps.
    if (this.size == null) return;
    if (!port.isVisible() && this.drag == null) {
      port.setBounds(this.placedBounds(), false);
      port.showInactive();
    }
  }

  /** The current glance, for a renderer that asks before the first push reaches it. */
  current(): GlancePush | null {
    return this.push;
  }

  handle(action: GlancePipAction): void {
    const { port } = this.deps;
    switch (action.type) {
      case 'open-engine':
        this.deps.openEngine();
        return;
      case 'open-session':
        this.deps.openSession(action.sessionId);
        return;
      case 'collapse':
        if (this.push) this.savePrefs({ ...this.push.prefs, desktopCollapsed: action.collapsed });
        return;
      case 'close':
        this.snoozed = true;
        this.refresh();
        return;
      case 'size': {
        const first = this.size == null;
        this.size = { width: Math.ceil(action.width), height: Math.ceil(action.height) };
        if (!port.exists() || this.drag != null) return;
        if (first) {
          this.refresh();
          return;
        }
        // A grown or shrunk card keeps its corner: the far edges move, the anchored ones stay.
        port.setBounds(this.placedBounds(), false);
        return;
      }
      case 'drag-start':
        if (!port.exists()) return;
        this.drag = {
          pointerX: action.screenX,
          pointerY: action.screenY,
          bounds: port.getBounds(),
          moved: false,
        };
        return;
      case 'drag-move': {
        const drag = this.drag;
        if (!drag) return;
        drag.moved = true;
        port.setBounds(
          {
            ...drag.bounds,
            x: Math.round(drag.bounds.x + action.screenX - drag.pointerX),
            y: Math.round(drag.bounds.y + action.screenY - drag.pointerY),
          },
          false
        );
        return;
      }
      case 'drag-end': {
        const drag = this.drag;
        this.drag = null;
        if (!drag?.moved || !this.push) return;
        // Released: snap to the nearest corner of the display it was dropped on, and remember both.
        const bounds = port.getBounds();
        const display = port.displayMatching(bounds);
        const corner = nearestCorner(bounds, display.workArea);
        this.savePrefs({ ...this.push.prefs, desktopPlace: { displayId: display.id, corner } });
        port.setBounds(this.boundsIn(display, corner), true);
        return;
      }
    }
  }

  /** A display went away or changed its work area: the window goes back to a corner that exists. */
  displaysChanged(): void {
    if (this.deps.port.exists() && this.deps.port.isVisible() && this.size != null) {
      this.deps.port.setBounds(this.placedBounds(), false);
    }
  }

  private savePrefs(next: GlancePrefs): void {
    if (this.push) this.push = { ...this.push, prefs: next };
    this.deps.savePrefs(next);
  }

  private boundsIn(display: GlanceDisplay, corner: GlanceCorner): Rect {
    return cornerBounds(
      corner,
      this.size ?? { width: 1, height: 1 },
      display.workArea,
      GLANCE_MARGIN
    );
  }

  /** The remembered corner of the remembered display — the primary display's when that one is gone. */
  private placedBounds(): Rect {
    const place = this.push?.prefs.desktopPlace ?? null;
    const display =
      (place && this.deps.port.displays().find((d) => d.id === place.displayId)) ||
      this.deps.port.primaryDisplay();
    return this.boundsIn(display, place?.corner ?? 'bottom-right');
  }
}
