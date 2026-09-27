import { BaseWindow, WebContentsView, screen, type Display, type Rectangle } from 'electron';
import type { GlanceDisplay, GlanceWindowPort } from './engineGlanceDesktop';

/**
 * The Electron window behind the desktop engine glance.
 *
 * A BaseWindow + WebContentsView, deliberately NOT a BrowserWindow: `BrowserWindow.getAllWindows()`
 * filters to BrowserWindows, and main has ~30 call sites (the app menu, `activate`, the tray's
 * action window, the quit guards, theme broadcasts) that must never pick the glance as "the goose
 * window". A BaseWindow cannot be picked by any of them.
 *
 * macOS: `type: 'panel'` adds NSWindowStyleMaskNonactivatingPanel — clicking it never activates goose
 * or takes focus from the app you are in — and lets it float over full-screen apps on every Space;
 * `focusable: false` keeps it from ever becoming the key window; `acceptFirstMouse` makes the first
 * click reach the card (it is never active, so every click is a first click); level `floating` keeps
 * it above normal windows and BELOW the Dock, menus and system alerts; `hiddenInMissionControl` keeps
 * Mission Control's overview clean. No vibrancy: a translucent blur is a faded fill, and the card is
 * a solid engine-phase colour.
 */

export interface GlanceWindowOptions {
  /** The renderer entry with `#/engine-glance`. */
  url: () => string;
  preload: string;
  additionalArguments: () => string[];
  /** The renderer's own IPC sender id, so main can tell its messages from a goose window's. */
  onWebContents: (id: number | null) => void;
}

function displayOf(d: Display): GlanceDisplay {
  return { id: d.id, workArea: d.workArea };
}

export function createGlanceWindowPort(options: GlanceWindowOptions): GlanceWindowPort {
  let win: BaseWindow | null = null;
  let view: WebContentsView | null = null;

  const layoutView = () => {
    if (!win || !view) return;
    const { width, height } = win.getContentBounds();
    view.setBounds({ x: 0, y: 0, width, height });
  };

  return {
    ensure() {
      if (win && !win.isDestroyed()) return;
      const mac = process.platform === 'darwin';
      win = new BaseWindow({
        show: false,
        frame: false,
        width: 300,
        height: 160,
        resizable: false,
        movable: true,
        minimizable: false,
        maximizable: false,
        fullscreenable: false,
        closable: true,
        skipTaskbar: true,
        focusable: false,
        acceptFirstMouse: true,
        hiddenInMissionControl: true,
        // The card draws its own rounded corners; the window around it is clear, and macOS casts
        // the shadow from the card's shape.
        transparent: true,
        backgroundColor: '#00000000',
        hasShadow: true,
        alwaysOnTop: true,
        ...(mac ? { type: 'panel' } : { type: 'toolbar' }),
      });
      win.setAlwaysOnTop(true, 'floating');
      // Every Space and over full-screen apps. skipTransformProcessType: goose stays a regular app —
      // the transform would hide the Dock icon and every window for a moment on each call.
      win.setVisibleOnAllWorkspaces(true, {
        visibleOnFullScreen: true,
        skipTransformProcessType: true,
      });
      view = new WebContentsView({
        webPreferences: {
          preload: options.preload,
          contextIsolation: true,
          nodeIntegration: false,
          webSecurity: true,
          partition: 'persist:goose',
          // The glance is read while goose is in the background: a throttled renderer would paint a
          // rate seconds late.
          backgroundThrottling: false,
          additionalArguments: options.additionalArguments(),
        },
      });
      view.setBackgroundColor('#00000000');
      win.contentView.addChildView(view);
      layoutView();
      win.on('resize', layoutView);
      const contents = view.webContents;
      options.onWebContents(contents.id);
      // A link inside the card never opens a window of its own.
      contents.setWindowOpenHandler(() => ({ action: 'deny' }));
      contents.on('will-navigate', (event) => event.preventDefault());
      win.on('closed', () => {
        win = null;
        view = null;
        options.onWebContents(null);
      });
      void contents.loadURL(options.url());
    },
    destroy() {
      if (view && !view.webContents.isDestroyed()) view.webContents.close();
      if (win && !win.isDestroyed()) win.destroy();
      win = null;
      view = null;
      options.onWebContents(null);
    },
    exists() {
      return win != null && !win.isDestroyed();
    },
    showInactive() {
      win?.showInactive();
    },
    hide() {
      win?.hide();
    },
    isVisible() {
      return win?.isVisible() ?? false;
    },
    getBounds() {
      return win?.getBounds() ?? { x: 0, y: 0, width: 0, height: 0 };
    },
    setBounds(bounds: Rectangle, animate: boolean) {
      if (!win) return;
      win.setBounds(bounds, animate);
      layoutView();
    },
    send(channel: string, payload: unknown) {
      if (view && !view.webContents.isDestroyed()) view.webContents.send(channel, payload);
    },
    displays() {
      return screen.getAllDisplays().map(displayOf);
    },
    displayMatching(rect: Rectangle) {
      return displayOf(screen.getDisplayMatching(rect));
    },
    cursorPoint() {
      return screen.getCursorScreenPoint();
    },
  };
}
