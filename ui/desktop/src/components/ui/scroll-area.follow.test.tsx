import { act, fireEvent, render } from '@testing-library/react';
import * as React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SCROLL_INTENT_EVENT } from '../../utils/userScroll';
import { ScrollArea, type ScrollAreaHandle } from './scroll-area';

/**
 * Q-496: the live chat stopped following its turn. Seen on the installed 3.0.78 — turn 4 running,
 * the pane parked on turn 2 (scrollTop 12,983 of ~39,485, 25,439 px above the bottom), and nobody
 * had scrolled: the only driver was a Playwright harness that fills the composer and presses Enter.
 *
 * jsdom lays nothing out and fires nothing on its own, so the viewport here is a small model of
 * Chromium's that the test drives frame by frame:
 *  - `content` is scrollHeight, `client` clientHeight; scrollTop clamps to [0, content - client];
 *  - a scrollTop change marks the viewport dirty and ONE scroll event fires at the next frame, the
 *    way the browser coalesces them — never synchronously on the assignment;
 *  - requestAnimationFrame callbacks run at the frame, after the scroll event;
 *  - a smooth scrollTo animates over SMOOTH_FRAMES frames to the target fixed when it STARTED,
 *    whatever the content does meanwhile; a plain assignment cancels it.
 * That half-finished animation is what the old follow logic mistook for the person scrolling up.
 */

const SMOOTH_FRAMES = 4;

type FrameCallback = Parameters<typeof requestAnimationFrame>[0];
interface ScrollOptions {
  top?: number;
  behavior?: string;
}

interface Metrics {
  content: number;
  client: number;
  top: number;
}

let metrics: Metrics;
let rafQueue: FrameCallback[];
let resizeCallbacks: Array<() => void>;
let scrollDirty: boolean;
let animation: { target: number; framesLeft: number } | null;
let activeViewport: HTMLDivElement | null;

const maxTop = () => Math.max(0, metrics.content - metrics.client);
const distanceFromBottom = () => metrics.content - metrics.top - metrics.client;

function setTop(value: number) {
  const next = Math.min(Math.max(0, value), maxTop());
  if (next === metrics.top) return;
  metrics.top = next;
  scrollDirty = true;
}

function installMetrics(viewport: HTMLDivElement) {
  activeViewport = viewport;
  Object.defineProperty(viewport, 'scrollHeight', {
    configurable: true,
    get: () => metrics.content,
  });
  Object.defineProperty(viewport, 'clientHeight', {
    configurable: true,
    get: () => metrics.client,
  });
  Object.defineProperty(viewport, 'scrollTop', {
    configurable: true,
    get: () => metrics.top,
    set: (value: number) => {
      animation = null;
      setTop(value);
    },
  });
  viewport.scrollTo = ((options: ScrollOptions) => {
    const target = Math.min(Math.max(0, options.top ?? 0), maxTop());
    if (options.behavior === 'smooth') {
      animation = { target, framesLeft: SMOOTH_FRAMES };
    } else {
      animation = null;
      setTop(target);
    }
  }) as typeof viewport.scrollTo;
}

/** One rendering step: the smooth animation advances, the scroll event fires, then rAF. */
function frame() {
  if (animation) {
    const step = (animation.target - metrics.top) / animation.framesLeft;
    setTop(Math.round(metrics.top + step));
    animation.framesLeft -= 1;
    if (animation.framesLeft === 0) animation = null;
  }
  if (scrollDirty && activeViewport) {
    scrollDirty = false;
    activeViewport.dispatchEvent(new Event('scroll'));
  }
  const callbacks = rafQueue;
  rafQueue = [];
  callbacks.forEach((cb) => cb(0));
}

function frames(n: number) {
  for (let i = 0; i < n; i += 1) act(() => frame());
}

/** Content changes size (a tool row lands, a failed row prints its error, a card folds). */
function resize(content: number) {
  metrics.content = content;
  if (metrics.top > maxTop()) setTop(maxTop());
}

function fireResizeObservers() {
  resizeCallbacks.forEach((cb) => cb());
}

interface Harness {
  viewport: HTMLDivElement;
  root: HTMLElement;
  handle: React.RefObject<ScrollAreaHandle | null>;
  follows: boolean[];
  rerender: (rows: number) => void;
}

function mount(rows = 3): Harness {
  const handle = React.createRef<ScrollAreaHandle>();
  const follows: boolean[] = [];
  const ui = (n: number) => (
    <ScrollArea ref={handle} autoScroll onScrollChange={(v) => follows.push(v)}>
      {Array.from({ length: n }, (_, i) => (
        <div key={i}>row {i}</div>
      ))}
    </ScrollArea>
  );
  const view = render(ui(rows));
  const viewport = view.container.querySelector<HTMLDivElement>(
    '[data-radix-scroll-area-viewport]'
  );
  if (!viewport) throw new Error('no viewport rendered');
  installMetrics(viewport);
  return {
    viewport,
    root: view.container.firstElementChild as HTMLElement,
    handle,
    follows,
    rerender: (n: number) => view.rerender(ui(n)),
  };
}

/** A render of the transcript with `grow` px more content (the list re-renders). */
function streamed(h: Harness, rows: number, grow: number) {
  act(() => {
    resize(metrics.content + grow);
    h.rerender(rows);
  });
}

function scrollUpBy(px: number) {
  act(() => {
    activeViewport!.scrollTop = metrics.top - px;
  });
}

const following = (h: Harness) => h.handle.current?.isFollowing;
const lastFollow = (h: Harness) => h.follows[h.follows.length - 1];

beforeEach(() => {
  metrics = { content: 2_000, client: 1_063, top: 2_000 - 1_063 };
  rafQueue = [];
  resizeCallbacks = [];
  scrollDirty = false;
  animation = null;
  activeViewport = null;
  vi.stubGlobal('requestAnimationFrame', (cb: FrameCallback) => {
    rafQueue.push(cb);
    return rafQueue.length;
  });
  vi.stubGlobal('cancelAnimationFrame', () => {});
  vi.stubGlobal(
    'ResizeObserver',
    class {
      private readonly cb: () => void;
      constructor(cb: () => void) {
        this.cb = () => cb();
        resizeCallbacks.push(this.cb);
      }
      observe() {}
      unobserve() {}
      disconnect() {
        resizeCallbacks = resizeCallbacks.filter((c) => c !== this.cb);
      }
    }
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('ScrollArea follow mode — content changes never end it (Q-496)', () => {
  it('a block landing mid-scroll, then more content: still at the live edge (the 3.0.78 park)', () => {
    const h = mount(3);
    // A tool group with its failed row and error line lands: far more than the old 200 px slack.
    streamed(h, 4, 1_800);
    frames(2);
    // The turn keeps streaming before the scroll to the first block has finished.
    streamed(h, 5, 900);
    frames(12);
    expect(h.follows).not.toContain(false);
    expect(following(h)).toBe(true);
    expect(distanceFromBottom()).toBe(0);
  });

  it('many turns of it — 152 tool rows, a notice, a new user message — never leave the edge', () => {
    const h = mount(3);
    for (let row = 4; row < 160; row += 1) {
      streamed(h, row, row % 7 === 0 ? 640 : 180);
      frames(row % 3);
    }
    frames(12);
    expect(h.follows).not.toContain(false);
    expect(distanceFromBottom()).toBe(0);
  });

  it('content that grows WITHOUT the list re-rendering (a card expanding) is followed', () => {
    const h = mount(3);
    act(() => {
      resize(metrics.content + 5_000);
      fireResizeObservers();
    });
    frames(6);
    expect(following(h)).toBe(true);
    expect(distanceFromBottom()).toBe(0);
  });

  it('a card folding above the fold (scroll anchoring moves scrollTop) keeps following', () => {
    const h = mount(3);
    streamed(h, 4, 30_000);
    frames(8);
    // Chromium's anchoring keeps the visible content still: scrollTop drops with the fold, and a
    // scroll event fires that no person caused.
    act(() => {
      resize(metrics.content - 600);
      setTop(metrics.top - 900);
      fireResizeObservers();
    });
    frames(3);
    streamed(h, 5, 400);
    frames(8);
    expect(h.follows).not.toContain(false);
    expect(distanceFromBottom()).toBe(0);
  });

  it('the viewport shrinking under it (a tray opening below) stays at the bottom', () => {
    const h = mount(3);
    act(() => {
      metrics.client -= 300;
      fireResizeObservers();
    });
    frames(3);
    expect(following(h)).toBe(true);
    expect(distanceFromBottom()).toBe(0);
  });
});

describe('ScrollArea follow mode — ends only on the person (Q-496)', () => {
  it('a wheel up ends follow; later content leaves the view where the person put it', () => {
    const h = mount(3);
    streamed(h, 4, 5_000);
    frames(8);
    act(() => {
      fireEvent.wheel(h.viewport, { deltaY: -120 });
    });
    scrollUpBy(120);
    frames(2);
    expect(lastFollow(h)).toBe(false);
    const parked = metrics.top;
    streamed(h, 5, 3_000);
    act(() => fireResizeObservers());
    frames(8);
    expect(metrics.top).toBe(parked);
    expect(following(h)).toBe(false);
  });

  it('a wheel DOWN, or a wheel a nested scroller takes, does not end follow', () => {
    const h = mount(3);
    act(() => {
      fireEvent.wheel(h.viewport, { deltaY: 120 });
    });
    const inner = document.createElement('div');
    inner.style.overflowY = 'auto';
    Object.defineProperty(inner, 'scrollHeight', { value: 900 });
    Object.defineProperty(inner, 'clientHeight', { value: 300 });
    Object.defineProperty(inner, 'scrollTop', { value: 200 });
    h.viewport.firstElementChild!.appendChild(inner);
    act(() => {
      fireEvent.wheel(inner, { deltaY: -120 });
    });
    frames(2);
    expect(h.follows).not.toContain(false);
    expect(following(h)).toBe(true);
  });

  it('a touch drag down (the content moves up) ends follow', () => {
    const h = mount(3);
    act(() => {
      fireEvent.touchStart(h.viewport, { touches: [{ clientY: 100 }] });
      fireEvent.touchMove(h.viewport, { touches: [{ clientY: 260 }] });
    });
    scrollUpBy(160);
    frames(2);
    expect(lastFollow(h)).toBe(false);
  });

  it('PageUp in the transcript ends follow; ArrowUp in a text field or PageUp in another pane does not', () => {
    const h = mount(3);
    const field = document.createElement('textarea');
    h.viewport.firstElementChild!.appendChild(field);
    act(() => {
      fireEvent.keyDown(field, { key: 'ArrowUp' });
    });
    expect(following(h)).toBe(true);
    const elsewhere = document.createElement('div');
    document.body.appendChild(elsewhere);
    act(() => {
      fireEvent.keyDown(elsewhere, { key: 'PageUp' });
    });
    expect(following(h)).toBe(true);
    act(() => {
      fireEvent.keyDown(h.viewport, { key: 'PageUp' });
    });
    scrollUpBy(800);
    frames(2);
    expect(lastFollow(h)).toBe(false);
    elsewhere.remove();
  });

  it('PageUp with focus on the body (after a click on transcript text) ends follow', () => {
    const h = mount(3);
    streamed(h, 4, 3_000);
    frames(4);
    act(() => {
      fireEvent.keyDown(document.body, { key: 'PageUp' });
    });
    expect(following(h)).toBe(false);
  });

  it('dragging the scrollbar up ends follow; a click in the transcript does not', () => {
    const h = mount(3);
    streamed(h, 4, 1_000);
    frames(4);
    act(() => {
      fireEvent.pointerDown(h.viewport, { button: 0 });
      fireEvent.pointerUp(window, { button: 0 });
    });
    frames(2);
    expect(following(h)).toBe(true);
    const scrollbar = document.createElement('div');
    h.root.appendChild(scrollbar);
    act(() => {
      fireEvent.pointerDown(scrollbar, { button: 0 });
    });
    scrollUpBy(500);
    frames(2);
    act(() => {
      fireEvent.pointerUp(window, { button: 0 });
    });
    expect(lastFollow(h)).toBe(false);
  });

  it('a user jump announced with SCROLL_INTENT_EVENT (a search result) ends follow', () => {
    const h = mount(3);
    act(() => {
      h.viewport.dispatchEvent(new Event(SCROLL_INTENT_EVENT));
    });
    scrollUpBy(400);
    frames(2);
    expect(lastFollow(h)).toBe(false);
    streamed(h, 4, 2_000);
    frames(8);
    expect(distanceFromBottom()).toBe(2_400);
  });

  it('scrolling back to the bottom resumes follow, and scrollToBottom (Jump to latest) does too', () => {
    const h = mount(3);
    streamed(h, 4, 5_000);
    frames(8);
    act(() => {
      fireEvent.wheel(h.viewport, { deltaY: -500 });
    });
    scrollUpBy(500);
    frames(2);
    expect(following(h)).toBe(false);
    act(() => {
      fireEvent.wheel(h.viewport, { deltaY: 500 });
    });
    scrollUpBy(-500);
    frames(2);
    expect(lastFollow(h)).toBe(true);
    act(() => {
      fireEvent.wheel(h.viewport, { deltaY: -2_000 });
    });
    scrollUpBy(2_000);
    frames(2);
    expect(following(h)).toBe(false);
    act(() => h.handle.current?.scrollToBottom());
    frames(8);
    expect(lastFollow(h)).toBe(true);
    expect(following(h)).toBe(true);
    expect(distanceFromBottom()).toBe(0);
  });
});
