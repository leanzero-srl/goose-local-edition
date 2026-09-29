import { BrowserWindow } from 'electron';
import log from './utils/logger';

/**
 * The app's own message dialog for the MAIN process — never a native OS message box. It runs where
 * no renderer may exist yet (goose serve failed, the external backend is unreachable, the first
 * window could not be made), so it is a small window of its own rendering a self-contained page.
 *
 * A button click navigates to a sentinel URL on the reserved `.invalid` TLD; the main side
 * intercepts it in `will-navigate`, cancels the navigation and resolves with the button's index.
 * Escape and closing the window both answer `cancelId`. It never rejects: a window that cannot be
 * made is logged and answered as the cancel path, which every caller treats as its safe exit.
 */

export type AppDialogTone = 'error' | 'info' | 'warning';

export interface AppDialogOptions {
  title: string;
  message: string;
  detail?: string;
  buttons: string[];
  defaultId?: number;
  cancelId?: number;
  tone: AppDialogTone;
  parent?: BrowserWindow | null;
}

const CHOICE_ORIGIN = 'https://app-dialog.invalid';
const CHOICE_PATH = /^\/choice\/(\d+)$/;

export const choiceUrl = (index: number): string => `${CHOICE_ORIGIN}/choice/${index}`;

/** The button index a sentinel URL names, or null for any other URL (or an index with no button). */
export function parseChoiceUrl(url: string, buttonCount: number): number | null {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.origin !== CHOICE_ORIGIN || parsed.search || parsed.hash) return null;
  const match = CHOICE_PATH.exec(parsed.pathname);
  if (!match) return null;
  const index = Number(match[1]);
  return Number.isSafeInteger(index) && index < buttonCount ? index : null;
}

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/** An out-of-range id is clamped to a real button, so Escape and close always name one. */
export function resolveDialogIds(
  options: Pick<AppDialogOptions, 'buttons' | 'defaultId' | 'cancelId'>
): {
  defaultId: number;
  cancelId: number;
} {
  const last = Math.max(options.buttons.length - 1, 0);
  const clamp = (id: number | undefined, fallback: number) =>
    id !== undefined && Number.isInteger(id) && id >= 0 && id <= last ? id : fallback;
  const defaultId = clamp(options.defaultId, 0);
  return { defaultId, cancelId: clamp(options.cancelId, last) };
}

const TONE_GLYPH: Record<AppDialogTone, string> = { error: '!', warning: '!', info: 'i' };

export function buildAppDialogHtml(options: AppDialogOptions): string {
  const { defaultId } = resolveDialogIds(options);
  const buttons = options.buttons
    .map((label, index) => {
      const isDefault = index === defaultId;
      return `<button type="button" class="${isDefault ? 'btn btn-default' : 'btn'}" data-choice="${index}"${
        isDefault ? ' autofocus' : ''
      }>${escapeHtml(label)}</button>`;
    })
    .join('');
  const detail = options.detail ? `<pre class="detail">${escapeHtml(options.detail)}</pre>` : '';

  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'">
<meta name="color-scheme" content="light dark">
<title>${escapeHtml(options.title)}</title>
<style>
:root {
  --surface: #ffffff; --text: #3f434b; --muted: #5b616b; --panel: #f4f6f7; --border: #cbd1d6;
  --primary-bg: #000000; --primary-text: #ffffff; --ring: #1d4ed8;
  --tone-error: #dc2626; --tone-info: #1d4ed8; --tone-warning: #f59e0b;
  --tone-error-text: #ffffff; --tone-info-text: #ffffff; --tone-warning-text: #000000;
}
@media (prefers-color-scheme: dark) {
  :root {
    --surface: #22252a; --text: #ffffff; --muted: #c2c7cd; --panel: #3f434b; --border: #525b68;
    --primary-bg: #cbd1d6; --primary-text: #000000; --ring: #60a5fa;
  }
}
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; }
body {
  background: var(--surface); color: var(--text); font: 13px/1.45 -apple-system, BlinkMacSystemFont,
  'Segoe UI', system-ui, sans-serif; -webkit-user-select: text; user-select: text;
}
header { display: flex; align-items: center; gap: 10px; padding: 14px 18px; background: var(--tone-bg); color: var(--tone-fg); }
body.tone-error { --tone-bg: var(--tone-error); --tone-fg: var(--tone-error-text); }
body.tone-info { --tone-bg: var(--tone-info); --tone-fg: var(--tone-info-text); }
body.tone-warning { --tone-bg: var(--tone-warning); --tone-fg: var(--tone-warning-text); }
.glyph {
  flex: none; width: 24px; height: 24px; border-radius: 50%; display: grid; place-items: center;
  background: var(--tone-fg); color: var(--tone-bg); font-weight: 800; font-size: 15px;
}
h1 { margin: 0; font-size: 15px; font-weight: 700; }
main { padding: 16px 18px 8px; }
.message { margin: 0 0 10px; font-size: 14px; font-weight: 600; white-space: pre-wrap; overflow-wrap: anywhere; }
.detail {
  margin: 0; padding: 10px 12px; max-height: 320px; overflow: auto; white-space: pre-wrap;
  overflow-wrap: anywhere; background: var(--panel); color: var(--muted); border: 1px solid var(--border);
  border-radius: 8px; font: 12px/1.45 ui-monospace, SFMono-Regular, Menlo, monospace;
}
footer { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; padding: 12px 18px 16px; }
.btn {
  appearance: none; cursor: pointer; padding: 7px 14px; border-radius: 8px; font: inherit; font-weight: 600;
  background: var(--surface); color: var(--text); border: 1px solid var(--border);
}
.btn-default { background: var(--primary-bg); color: var(--primary-text); border-color: var(--primary-bg); }
.btn:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; }
</style>
</head>
<body class="tone-${options.tone}">
<header><span class="glyph" aria-hidden="true">${TONE_GLYPH[options.tone]}</span><h1>${escapeHtml(
    options.title
  )}</h1></header>
<main role="alertdialog" aria-labelledby="m"><p class="message" id="m">${escapeHtml(
    options.message
  )}</p>${detail}</main>
<footer>${buttons}</footer>
<script>
document.querySelectorAll('button[data-choice]').forEach(function (b) {
  b.addEventListener('click', function () {
    location.href = '${CHOICE_ORIGIN}/choice/' + b.getAttribute('data-choice');
  });
});
var d = document.querySelector('button[autofocus]');
if (d) d.focus();
</script>
</body>
</html>`;
}

// A dialog's window closing can be the last window closing; the app's `window-all-closed` handler
// asks this so a dialog's answer (not its window) decides whether the app quits. Electron emits the
// window's `closed` BEFORE `window-all-closed`, so the count drops one macrotask after `closed`.
let dialogsInFlight = 0;
export const isAppDialogInFlight = (): boolean => dialogsInFlight > 0;

const DIALOG_WIDTH = 480;
const MAX_CONTENT_HEIGHT = 640;

export function showAppDialog(options: AppDialogOptions): Promise<number> {
  const { cancelId } = resolveDialogIds(options);
  let win: BrowserWindow;
  try {
    const parent = options.parent && !options.parent.isDestroyed() ? options.parent : undefined;
    win = new BrowserWindow({
      width: DIALOG_WIDTH,
      height: 260,
      useContentSize: true,
      show: false,
      resizable: false,
      minimizable: false,
      maximizable: false,
      fullscreenable: false,
      title: options.title,
      parent,
      modal: Boolean(parent),
      webPreferences: {
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: true,
        spellcheck: false,
      },
    });
  } catch (error) {
    log.error(`[app-dialog] could not open "${options.title}"; answering cancel:`, error);
    return Promise.resolve(cancelId);
  }

  dialogsInFlight += 1;
  return new Promise<number>((resolve) => {
    let settled = false;
    const settle = (choice: number) => {
      if (settled) return;
      settled = true;
      resolve(choice);
      if (!win.isDestroyed()) win.close();
    };

    win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    win.webContents.on('will-navigate', (event, url) => {
      event.preventDefault();
      const choice = parseChoiceUrl(url, options.buttons.length);
      if (choice !== null) settle(choice);
    });
    win.webContents.on('before-input-event', (event, input) => {
      if (input.type === 'keyDown' && input.key === 'Escape') {
        event.preventDefault();
        settle(cancelId);
      }
    });
    win.once('closed', () => {
      setTimeout(() => {
        dialogsInFlight -= 1;
      }, 0);
      settle(cancelId);
    });
    win.webContents.once('did-finish-load', async () => {
      try {
        // The body's own height, not the document's: a document is never shorter than its viewport,
        // so measuring it could only grow the window, never fit it to a short message.
        const height = Number(
          await win.webContents.executeJavaScript(
            'document.body.getBoundingClientRect().height',
            true
          )
        );
        if (!win.isDestroyed() && Number.isFinite(height) && height > 0) {
          win.setContentSize(DIALOG_WIDTH, Math.min(Math.ceil(height), MAX_CONTENT_HEIGHT));
        }
      } catch (error) {
        log.warn('[app-dialog] could not measure the dialog; keeping its default size:', error);
      }
      if (!win.isDestroyed()) {
        win.show();
        win.focus();
      }
    });

    const html = buildAppDialogHtml(options);
    win
      .loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(html)}`)
      .catch((error: unknown) => {
        log.error(`[app-dialog] could not load "${options.title}"; answering cancel:`, error);
        settle(cancelId);
      });
  });
}
