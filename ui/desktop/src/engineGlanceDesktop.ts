import {
  DEFAULT_GLANCE_CORNER,
  ENGINE_GLANCE_CHANNEL,
  type GlanceCorner,
  type GlancePrefs,
  type GlancePush,
} from './utils/engineGlance';
import {
  clearOfGoose,
  cornerBounds,
  desktopGlanceVisible,
  glanceLive,
  gooseOnScreen,
  nearestCorner,
  placeGlance,
  snoozeAfter,
  workingDisplay,
  type GlanceDisplay,
  type GlancePlacement,
  type GooseWindowFacts,
  type Rect,
} from './utils/engineGlanceRules';

export type { GlanceDisplay };

/**
 * The engine glance as a SYSTEM-LEVEL floating mini window: shown while something is live and no
 * goose window can be seen (or whenever something is live, by choice), in a corner of the display
 * the person is working on, over every app and every full-screen Space, never taking focus and
 * never over a goose window that can be seen (Q-226).
 *
 * The controller is Electron-free — main hands it a `GlanceWindowPort` (engineGlanceWindow.ts) — so
 * every transition (show inactive, snooze, snap, remember the display) is tested as data.
 */

/** What the glance's renderer asks main to do. */
export type GlancePipAction =
  | { type: 'open-engine' }
  | { type: 'open-session'; sessionId: string }
  | { type: 'collapse'; collapsed: boolean }
  /** "Hide for now": snoozed for this live spell. */
  | { type: 'close' }
  /** "Turn off the floating window": the setting to Off, as Settings › App writes it (Q-224). */
  | { type: 'turn-off' }
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
    case 'turn-off':
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
  /** Where the pointer is, in screen coordinates. */
  cursorPoint(): { x: number; y: number };
}

export interface GlanceDesktopDeps {
  port: GlanceWindowPort;
  /** `process.platform`: occlusion is read on macOS only (engineGlanceRules.ts `gooseOnScreen`). */
  platform: string;
  /** Every goose window (never the glance): can it be seen, is it focused, where is it. */
  gooseWindows(): GooseWindowFacts[];
  savePrefs(next: GlancePrefs): void;
  openEngine(): void;
  openSession(sessionId: string): void;
  /**
   * Tell the goose window in front that the person turned the desktop window off from it (Q-224).
   * false = no goose window is in front to say it (goose is in the background); asked again the
   * next time something is re-decided, which is when a goose window comes to the front.
   */
  tellTurnedOff(): boolean;
}

/** px from the work area's edges — the gap macOS leaves around its own Picture in Picture. */
export const GLANCE_MARGIN = 16;

export class EngineGlanceDesktop {
  private push: GlancePush | null = null;
  private snoozed = false;
  private turnedOffUntold = false;
  private size: { width: number; height: number } | null = null;
  /** Where it is showing: the display and corner it was placed in, kept while it stays up. */
  private place: { displayId: number; corner: GlanceCorner } | null = null;
  private drag: { pointerX: number; pointerY: number; bounds: Rect; moved: boolean } | null = null;
  /**
   * The app is quitting (Q-229). Electron's quit closes every window and finishes only when the
   * window list is EMPTY; this controller used to make the window again on the very next refresh
   * — the goose window's close/blur, or the next engine snapshot — so the quit never finished, and
   * since Electron's quit was already under way every later quit (and SIGTERM, which is a quit)
   * returned at once. While quitting, nothing here makes or shows a window.
   */
  private quitting = false;

  constructor(private readonly deps: GlanceDesktopDeps) {}

  /** The quit began: the window goes now, and nothing makes it again. */
  suspendForQuit(): void {
    this.quitting = true;
    this.drag = null;
    if (this.deps.port.exists()) this.deps.port.destroy();
  }

  /** The quit was refused (a live run's close guard): the window may come back. */
  resumeAfterRefusedQuit(): void {
    if (!this.quitting) return;
    this.quitting = false;
    this.refresh();
  }

  /** The latest glance: pushed to the window, then shown or hidden by the rules. */
  update(push: GlancePush): void {
    this.push = push;
    this.snoozed = snoozeAfter(this.snoozed, push);
    if (this.deps.port.exists()) this.deps.port.send(ENGINE_GLANCE_CHANNEL, push);
    this.refresh();
  }

  /** Re-decide after a fact the glance does not carry changed (a goose window was covered, or not). */
  refresh(): void {
    if (this.quitting) return;
    if (this.turnedOffUntold && this.deps.tellTurnedOff()) this.turnedOffUntold = false;
    const { port } = this.deps;
    const push = this.push;
    const windows = this.deps.gooseWindows();
    const visible =
      push != null &&
      desktopGlanceVisible(push, {
        gooseOnScreen: gooseOnScreen(this.deps.platform, windows),
        snoozed: this.snoozed,
      });
    if (!visible) {
      if (port.exists() && port.isVisible()) port.hide();
      // Turned off: nothing floats and nothing is kept alive for it.
      if (push?.prefs.desktop === 'off' && port.exists()) port.destroy();
      // Live while goose is in sight: the window is made (hidden) now, so it shows the moment goose
      // is covered instead of after a cold renderer loads (measured ~4 s on the packaged build).
      else if (push != null && push.prefs.desktop !== 'off' && glanceLive(push) && !port.exists()) {
        port.ensure();
        port.send(ENGINE_GLANCE_CHANNEL, push);
      }
      return;
    }
    if (!port.exists()) {
      port.ensure();
      port.send(ENGINE_GLANCE_CHANNEL, push);
    }
    // First show waits for the renderer's measured size: a window shown at a guessed size jumps.
    if (this.size == null) return;
    if (this.drag != null) return;
    if (!port.isVisible()) {
      const placement = this.placement(windows);
      // Every spot is over a goose window that can be seen (only by choice, "whenever the engine
      // works"): it waits hidden until one is clear.
      if (!placement) return;
      this.placeAt(placement, false);
      port.showInactive();
      // It has appeared: its one-time "you can turn this off from here" has had its showing.
      if (!push.prefs.desktopHintSeen) this.savePrefs({ ...push.prefs, desktopHintSeen: true });
      return;
    }
    // Up, and a goose window came into view under it: it moves off, or goes.
    if (!clearOfGoose(port.getBounds(), windows)) {
      const placement = this.placement(windows);
      if (placement) this.placeAt(placement, true);
      else port.hide();
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
      case 'turn-off':
        if (!this.push) return;
        this.turnedOffUntold = true;
        // main's save republishes the glance, whose refresh hides and destroys the window.
        this.savePrefs({ ...this.push.prefs, desktop: 'off' });
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
        // A grown or shrunk card keeps its display and corner: the far edges move, the anchored
        // ones stay — even when the pointer has since gone to another display.
        this.keepPlace(false);
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
        this.place = { displayId: display.id, corner };
        port.setBounds(this.boundsIn(display, corner), true);
        return;
      }
    }
  }

  /** A display went away or changed its work area: the window goes back to a corner that exists. */
  displaysChanged(): void {
    if (this.deps.port.exists() && this.deps.port.isVisible() && this.size != null) {
      this.keepPlace(false);
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

  /** Where it shows now: the working display, the remembered corner, clear of goose (rules). */
  private placement(windows: GooseWindowFacts[]): GlancePlacement | null {
    const { port } = this.deps;
    const displays = port.displays();
    return placeGlance({
      displays,
      working: workingDisplay(displays, windows, port.cursorPoint()),
      remembered: this.push?.prefs.desktopPlace ?? null,
      defaultCorner: DEFAULT_GLANCE_CORNER,
      windows,
      size: this.size ?? { width: 1, height: 1 },
      margin: GLANCE_MARGIN,
    });
  }

  private placeAt(placement: GlancePlacement, animate: boolean): void {
    this.place = { displayId: placement.displayId, corner: placement.corner };
    this.deps.port.setBounds(placement.bounds, animate);
  }

  /** Re-fit in the display and corner it shows in; placed afresh only when that display is gone. */
  private keepPlace(animate: boolean): void {
    const { port } = this.deps;
    const display = this.place && port.displays().find((d) => d.id === this.place?.displayId);
    if (display && this.place) {
      port.setBounds(this.boundsIn(display, this.place.corner), animate);
      return;
    }
    const placement = this.placement(this.deps.gooseWindows());
    if (placement) this.placeAt(placement, animate);
    else port.hide();
  }
}
