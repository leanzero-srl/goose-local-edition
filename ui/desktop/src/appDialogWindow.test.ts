import { EventEmitter } from 'node:events';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { testClock } from './test/testClock';

const created: FakeWindow[] = [];
let constructorError: Error | null = null;

class FakeWebContents extends EventEmitter {
  setWindowOpenHandler = vi.fn();
  executeJavaScript = vi.fn(async () => 200);
}

class FakeWindow extends EventEmitter {
  webContents = new FakeWebContents();
  destroyed = false;
  options: unknown;
  loadedUrl = '';
  show = vi.fn();
  focus = vi.fn();
  setContentSize = vi.fn();
  constructor(options: unknown) {
    super();
    if (constructorError) throw constructorError;
    this.options = options;
    created.push(this);
  }
  isDestroyed() {
    return this.destroyed;
  }
  close() {
    if (this.destroyed) return;
    this.destroyed = true;
    this.emit('closed');
  }
  loadURL(url: string) {
    this.loadedUrl = url;
    return Promise.resolve();
  }
}

vi.mock('electron', () => ({ BrowserWindow: FakeWindow }));
vi.mock('./utils/logger', () => ({ default: { error: vi.fn(), warn: vi.fn(), info: vi.fn() } }));

const {
  buildAppDialogHtml,
  choiceUrl,
  escapeHtml,
  isAppDialogInFlight,
  parseChoiceUrl,
  resolveDialogIds,
  showAppDialog,
} = await import('./appDialogWindow');
const log = (await import('./utils/logger')).default;

const navigate = (win: FakeWindow, url: string) => {
  const event = { preventDefault: vi.fn() };
  win.webContents.emit('will-navigate', event, url);
  return event;
};

describe('buildAppDialogHtml', () => {
  it('renders hostile message, detail, title and labels inert', () => {
    const hostile = `<script>alert("x")</script> it's "quoted" & <img src=x onerror=alert(1)>`;
    const html = buildAppDialogHtml({
      tone: 'error',
      title: hostile,
      message: hostile,
      detail: hostile,
      buttons: [hostile, 'Quit'],
    });
    const doc = new DOMParser().parseFromString(html, 'text/html');
    // The page's own click wiring is the ONE script; nothing from the text became markup.
    expect(doc.querySelectorAll('script')).toHaveLength(1);
    expect(doc.querySelectorAll('img')).toHaveLength(0);
    expect(doc.querySelector('.message')?.textContent).toBe(hostile);
    expect(doc.querySelector('.detail')?.textContent).toBe(hostile);
    expect(doc.querySelector('h1')?.textContent).toBe(hostile);
    expect(doc.title).toBe(hostile);
    expect(doc.querySelector('button[data-choice="0"]')?.textContent).toBe(hostile);
  });

  it('keeps button order and ids, and focuses the default button', () => {
    const html = buildAppDialogHtml({
      tone: 'info',
      title: 'Update Ready to Install',
      message: 'm',
      buttons: ['Open Folder & Quit', 'Open Folder Only', 'Cancel'],
      defaultId: 1,
      cancelId: 2,
    });
    const doc = new DOMParser().parseFromString(html, 'text/html');
    const buttons = [...doc.querySelectorAll('button')];
    expect(buttons.map((b) => [b.dataset.choice, b.textContent])).toEqual([
      ['0', 'Open Folder & Quit'],
      ['1', 'Open Folder Only'],
      ['2', 'Cancel'],
    ]);
    expect(buttons.filter((b) => b.hasAttribute('autofocus')).map((b) => b.dataset.choice)).toEqual(
      ['1']
    );
    expect(doc.body.className).toBe('tone-info');
  });

  it('omits the detail block when there is no detail', () => {
    const html = buildAppDialogHtml({ tone: 'warning', title: 't', message: 'm', buttons: ['OK'] });
    expect(new DOMParser().parseFromString(html, 'text/html').querySelector('.detail')).toBeNull();
  });

  it('respects the colour scheme', () => {
    const html = buildAppDialogHtml({ tone: 'error', title: 't', message: 'm', buttons: ['OK'] });
    expect(html).toContain('@media (prefers-color-scheme: dark)');
  });
});

describe('escapeHtml', () => {
  it('escapes every markup-significant character', () => {
    expect(escapeHtml(`<a href="x" title='y'>&</a>`)).toBe(
      '&lt;a href=&quot;x&quot; title=&#39;y&#39;&gt;&amp;&lt;/a&gt;'
    );
  });
});

describe('resolveDialogIds', () => {
  it('uses the given ids when they name real buttons', () => {
    expect(resolveDialogIds({ buttons: ['a', 'b'], defaultId: 0, cancelId: 1 })).toEqual({
      defaultId: 0,
      cancelId: 1,
    });
  });

  it('defaults to the first button and cancels to the last', () => {
    expect(resolveDialogIds({ buttons: ['a', 'b', 'c'] })).toEqual({ defaultId: 0, cancelId: 2 });
  });

  it('clamps an id with no button', () => {
    expect(resolveDialogIds({ buttons: ['Quit'], defaultId: 3, cancelId: -1 })).toEqual({
      defaultId: 0,
      cancelId: 0,
    });
  });
});

describe('parseChoiceUrl', () => {
  it('reads the index from the sentinel URL', () => {
    expect(parseChoiceUrl(choiceUrl(0), 2)).toBe(0);
    expect(parseChoiceUrl(choiceUrl(1), 2)).toBe(1);
  });

  it('is null for any URL that is not a choice', () => {
    for (const url of [
      'https://example.com/choice/0',
      'https://app-dialog.invalid/choice/x',
      'https://app-dialog.invalid/choice/0?x=1',
      'https://app-dialog.invalid/choice/0#y',
      'https://app-dialog.invalid/other/0',
      'http://app-dialog.invalid/choice/0',
      'data:text/html,hi',
      'not a url',
      '',
    ]) {
      expect(parseChoiceUrl(url, 2), url).toBeNull();
    }
  });

  it('is null for an index with no button', () => {
    expect(parseChoiceUrl(choiceUrl(2), 2)).toBeNull();
  });
});

describe('showAppDialog', () => {
  beforeEach(() => {
    created.length = 0;
    constructorError = null;
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.runAllTimers();
    vi.useRealTimers();
  });

  const options = {
    tone: 'error' as const,
    title: 'External Backend Unreachable',
    message: 'Could not connect',
    buttons: ['Disable External Backend & Retry', 'Quit'],
    defaultId: 0,
    cancelId: 1,
  };

  it('opens an isolated window on a data URL of the built page', async () => {
    void showAppDialog(options);
    const win = created[0];
    expect(win.options).toMatchObject({
      show: false,
      webPreferences: { contextIsolation: true, nodeIntegration: false },
    });
    expect(win.loadedUrl.startsWith('data:text/html;charset=utf-8,')).toBe(true);
    expect(decodeURIComponent(win.loadedUrl.split(',').slice(1).join(','))).toBe(
      buildAppDialogHtml(options)
    );
    win.close();
  });

  it('resolves the clicked button once, cancels the navigation and closes the window', async () => {
    const answer = showAppDialog(options);
    const win = created[0];
    const event = navigate(win, choiceUrl(0));
    expect(event.preventDefault).toHaveBeenCalled();
    expect(win.isDestroyed()).toBe(true);
    navigate(win, choiceUrl(1));
    await expect(answer).resolves.toBe(0);
  });

  it('refuses any other navigation without answering', async () => {
    const answer = showAppDialog(options);
    const win = created[0];
    const event = navigate(win, 'https://example.com/');
    expect(event.preventDefault).toHaveBeenCalled();
    expect(win.isDestroyed()).toBe(false);
    navigate(win, choiceUrl(1));
    await expect(answer).resolves.toBe(1);
  });

  it('answers cancelId when Escape is pressed', async () => {
    const answer = showAppDialog(options);
    const win = created[0];
    const event = { preventDefault: vi.fn() };
    win.webContents.emit('before-input-event', event, { type: 'keyDown', key: 'Escape' });
    expect(event.preventDefault).toHaveBeenCalled();
    await expect(answer).resolves.toBe(1);
  });

  it('answers cancelId when the window is closed', async () => {
    const answer = showAppDialog(options);
    created[0].close();
    await expect(answer).resolves.toBe(1);
  });

  it('holds window-all-closed off until one tick after its window closes', async () => {
    const answer = showAppDialog(options);
    expect(isAppDialogInFlight()).toBe(true);
    navigate(created[0], choiceUrl(0));
    await answer;
    expect(isAppDialogInFlight()).toBe(true);
    vi.runAllTimers();
    expect(isAppDialogInFlight()).toBe(false);
  });

  it('logs and answers cancelId when no window can be made, never a native box', async () => {
    constructorError = new Error('no display');
    await expect(showAppDialog(options)).resolves.toBe(1);
    expect(log.error).toHaveBeenCalled();
    expect(isAppDialogInFlight()).toBe(false);
  });

  it('sizes the window to its content before showing it', async () => {
    void showAppDialog(options);
    const win = created[0];
    win.webContents.emit('did-finish-load');
    await vi.waitFor(() => expect(win.show).toHaveBeenCalled(), { timeout: testClock() });
    expect(win.setContentSize).toHaveBeenCalledWith(480, 200);
    win.close();
  });
});
