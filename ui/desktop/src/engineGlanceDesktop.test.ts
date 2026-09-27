import { describe, expect, it, vi } from 'vitest';
import {
  EngineGlanceDesktop,
  GLANCE_MARGIN,
  isGlancePipAction,
  type GlanceDisplay,
  type GlanceWindowPort,
} from './engineGlanceDesktop';
import { ENGINE_GLANCE_CHANNEL, type GlancePrefs, type GlancePush } from './utils/engineGlance';
import type { GooseWindowFacts, Rect } from './utils/engineGlanceRules';
import { glancePush, runningSnapshot } from './utils/engineGlance.fixtures';
import { GENERATING_STATUS, IDLE_STATUS } from './components/leanzero-swarm/mlxLiveStatus.fixtures';

const MAIN: GlanceDisplay = { id: 1, workArea: { x: 0, y: 25, width: 1512, height: 920 } };
const SIDE: GlanceDisplay = { id: 2, workArea: { x: 1512, y: 0, width: 1920, height: 1080 } };

function fakePort(displays: GlanceDisplay[] = [MAIN]) {
  const state = {
    exists: false,
    visible: false,
    bounds: { x: 0, y: 0, width: 300, height: 160 } as Rect,
    sent: [] as unknown[],
    calls: [] as string[],
    displays,
    cursor: { x: 700, y: 400 },
  };
  const port: GlanceWindowPort = {
    ensure: () => {
      if (!state.exists) state.calls.push('ensure');
      state.exists = true;
    },
    destroy: () => {
      state.calls.push('destroy');
      state.exists = false;
      state.visible = false;
    },
    exists: () => state.exists,
    showInactive: () => {
      state.calls.push('showInactive');
      state.visible = true;
    },
    hide: () => {
      state.calls.push('hide');
      state.visible = false;
    },
    isVisible: () => state.visible,
    getBounds: () => state.bounds,
    setBounds: (b) => {
      state.bounds = b;
    },
    send: (channel, payload) => {
      if (channel === ENGINE_GLANCE_CHANNEL) state.sent.push(payload);
    },
    displays: () => state.displays,
    displayMatching: (r) =>
      state.displays.find(
        (d) =>
          r.x + r.width / 2 >= d.workArea.x && r.x + r.width / 2 < d.workArea.x + d.workArea.width
      ) ?? state.displays[0],
    cursorPoint: () => state.cursor,
  };
  return { port, state };
}

/** goose's window, in the middle of MAIN: every corner of MAIN stays clear of it. */
const GOOSE_BOUNDS: Rect = { x: 400, y: 200, width: 600, height: 500 };

function setup(opts: { inFront?: boolean; displays?: GlanceDisplay[] } = {}) {
  const { port, state } = fakePort(opts.displays);
  // inFront: goose's window in view and focused; else it is out of sight (covered, another Space,
  // minimized). `windows` overrides both for the Q-226 cases.
  const facts: { inFront: boolean; windows: GooseWindowFacts[] | null } = {
    inFront: opts.inFront ?? false,
    windows: null,
  };
  const saved: GlancePrefs[] = [];
  const openEngine = vi.fn();
  const openSession = vi.fn();
  // The goose window in front hears "turned off" only while goose IS in front (main's rule).
  const told: string[] = [];
  const desktop = new EngineGlanceDesktop({
    port,
    platform: 'darwin',
    gooseWindows: () =>
      facts.windows ?? [{ onScreen: facts.inFront, focused: facts.inFront, bounds: GOOSE_BOUNDS }],
    savePrefs: (p) => saved.push(p),
    openEngine,
    openSession,
    tellTurnedOff: () => {
      if (!facts.inFront) return false;
      told.push('turned-off');
      return true;
    },
  });
  return { desktop, state, facts, saved, told, openEngine, openSession };
}

const writing = glancePush(runningSnapshot(GENERATING_STATUS));
const idle = glancePush(runningSnapshot(IDLE_STATUS));
const withPrefs = (push: GlancePush, prefs: Partial<GlancePrefs>): GlancePush => ({
  ...push,
  prefs: { ...push.prefs, ...prefs },
});

describe('EngineGlanceDesktop — the floating window’s life', () => {
  it('live and goose in the background: created, sized by its renderer, then shown INACTIVE in the bottom-right', () => {
    const { desktop, state } = setup();
    desktop.update(writing);
    expect(state.calls).toEqual(['ensure']);
    // Never shown at a guessed size: it waits for the renderer's measurement.
    expect(state.visible).toBe(false);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.calls).toEqual(['ensure', 'showInactive']);
    expect(state.bounds).toEqual({
      x: 1512 - GLANCE_MARGIN - 300,
      y: 25 + 920 - GLANCE_MARGIN - 180,
      width: 300,
      height: 180,
    });
  });

  it('goose comes to the front: hidden; it leaves again: shown again, never focused', () => {
    const { desktop, state, facts } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    facts.inFront = true;
    desktop.refresh();
    expect(state.visible).toBe(false);
    facts.inFront = false;
    desktop.refresh();
    expect(state.visible).toBe(true);
    expect(state.calls.filter((c) => c === 'showInactive')).toHaveLength(2);
  });

  it('live while goose is in front: made hidden and warm, then shown the moment goose leaves', () => {
    const { desktop, state, facts } = setup({ inFront: true });
    desktop.update(writing);
    expect(state.calls).toEqual(['ensure']);
    expect(state.visible).toBe(false);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(false);
    facts.inFront = false;
    desktop.refresh();
    expect(state.calls).toEqual(['ensure', 'showInactive']);
  });

  it('nothing live: no window is made at all', () => {
    const { desktop, state } = setup({ inFront: true });
    desktop.update(idle);
    expect(state.calls).toEqual([]);
  });

  it('the engine goes quiet: hidden', () => {
    const { desktop, state } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.update(idle);
    expect(state.visible).toBe(false);
  });

  it('closed: stays closed while the spell lasts, and comes back with the next one', () => {
    const { desktop, state } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.handle({ type: 'close' });
    expect(state.visible).toBe(false);
    desktop.update(writing);
    expect(state.visible).toBe(false);
    desktop.update(idle);
    desktop.update(writing);
    expect(state.visible).toBe(true);
  });

  it('turned off: hidden and destroyed — nothing kept alive for it', () => {
    const { desktop, state } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.update(withPrefs(writing, { desktop: 'off' }));
    expect(state.exists).toBe(false);
  });

  it('every glance reaches the window once it exists', () => {
    const { desktop, state } = setup();
    desktop.update(idle);
    expect(state.sent).toHaveLength(0);
    desktop.update(writing);
    desktop.update(writing);
    expect(state.sent.length).toBeGreaterThanOrEqual(2);
  });

  it('a drag moves it with the pointer, and the release snaps it to the nearest corner of THAT display and remembers both', () => {
    const { desktop, state, saved } = setup({ displays: [MAIN, SIDE] });
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    const start = state.bounds;
    desktop.handle({ type: 'drag-start', screenX: 1300, screenY: 800 });
    desktop.handle({ type: 'drag-move', screenX: 1300 + 1000, screenY: 800 - 700 });
    expect(state.bounds.x).toBe(start.x + 1000);
    desktop.handle({ type: 'drag-end' });
    expect(saved[saved.length - 1]?.desktopPlace).toEqual({ displayId: 2, corner: 'top-left' });
    expect(state.bounds).toEqual({
      x: 1512 + GLANCE_MARGIN,
      y: GLANCE_MARGIN,
      width: 300,
      height: 180,
    });
  });

  it('a press that never moved is not a drag: nothing saved, nothing snapped', () => {
    const { desktop, saved } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    const before = saved.length;
    desktop.handle({ type: 'drag-start', screenX: 10, screenY: 10 });
    desktop.handle({ type: 'drag-end' });
    expect(saved).toHaveLength(before);
  });

  it('a remembered display that is gone: the same corner of the display the person works on', () => {
    const { desktop, state } = setup();
    desktop.update(withPrefs(writing, { desktopPlace: { displayId: 99, corner: 'top-left' } }));
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.bounds).toEqual({
      x: GLANCE_MARGIN,
      y: 25 + GLANCE_MARGIN,
      width: 300,
      height: 180,
    });
  });

  it('the card grows (details opened): it keeps its corner', () => {
    const { desktop, state } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.handle({ type: 'size', width: 300, height: 320 });
    expect(state.bounds.y + state.bounds.height).toBe(25 + 920 - GLANCE_MARGIN);
  });

  it('collapse is stored; clicks open the engine or the chat through main', () => {
    const { desktop, saved, openEngine, openSession } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'collapse', collapsed: true });
    expect(saved[saved.length - 1]?.desktopCollapsed).toBe(true);
    desktop.handle({ type: 'open-engine' });
    desktop.handle({ type: 'open-session', sessionId: 's1' });
    expect(openEngine).toHaveBeenCalledOnce();
    expect(openSession).toHaveBeenCalledWith('s1');
  });
});

describe('EngineGlanceDesktop — Q-224: turned off from the window itself', () => {
  it('"Turn off the floating window": the setting to Off through the one save, the window destroyed', () => {
    const { desktop, state, saved } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(true);
    desktop.handle({ type: 'turn-off' });
    const last = saved[saved.length - 1];
    expect(last).toEqual({ ...writing.prefs, desktopHintSeen: true, desktop: 'off' });
    expect(state.visible).toBe(false);
    expect(state.exists).toBe(false);
    // main republishes with the saved prefs: nothing comes back while it is Off, live or not.
    desktop.update(withPrefs(writing, last));
    expect(state.calls.filter((c) => c === 'ensure')).toHaveLength(1);
  });

  it('"Hide for now" writes nothing: the setting stays as it was', () => {
    const { desktop, saved } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    const before = saved.length;
    desktop.handle({ type: 'close' });
    expect(saved).toHaveLength(before);
  });

  it('goose in the background at the click: the notice waits for a goose window in front, once', () => {
    const { desktop, facts, told } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.handle({ type: 'turn-off' });
    expect(told).toEqual([]);
    // Every snapshot re-decides; the person is still in the other app.
    desktop.refresh();
    expect(told).toEqual([]);
    facts.inFront = true;
    desktop.refresh();
    expect(told).toEqual(['turned-off']);
    desktop.refresh();
    expect(told).toEqual(['turned-off']);
  });

  it('goose in front at the click ("whenever the engine works"): told at once', () => {
    const { desktop, told } = setup({ inFront: true });
    desktop.update(withPrefs(writing, { desktop: 'busy' }));
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.handle({ type: 'turn-off' });
    expect(told).toEqual(['turned-off']);
  });

  it('turned off from Settings › App: no notice — the person is already looking at the way back', () => {
    const { desktop, facts, told } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.update(withPrefs(writing, { desktop: 'off' }));
    facts.inFront = true;
    desktop.refresh();
    expect(told).toEqual([]);
  });
});

describe('EngineGlanceDesktop — Q-224: the one-time hint', () => {
  it('the first time the window ever appears, it is marked seen — once, and not before it shows', () => {
    const { desktop, facts, saved } = setup({ inFront: true });
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    // Made warm and hidden while goose is in front: not seen yet.
    expect(saved.filter((p) => p.desktopHintSeen)).toHaveLength(0);
    facts.inFront = false;
    desktop.refresh();
    expect(saved.filter((p) => p.desktopHintSeen)).toHaveLength(1);
    // Shown again later: nothing more to mark.
    facts.inFront = true;
    desktop.refresh();
    facts.inFront = false;
    desktop.refresh();
    expect(saved.filter((p) => p.desktopHintSeen)).toHaveLength(1);
  });

  it('already seen (stored): nothing is written when it shows', () => {
    const { desktop, saved } = setup();
    desktop.update(withPrefs(writing, { desktopHintSeen: true }));
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(saved).toHaveLength(0);
  });
});

describe('EngineGlanceDesktop — Q-226: only when goose cannot be seen, where the person works', () => {
  it('the owner’s screenshot: goose in plain view, another app focused on another display — no card', () => {
    const { desktop, state, facts } = setup({ displays: [MAIN, SIDE] });
    facts.windows = [{ onScreen: true, focused: false, bounds: GOOSE_BOUNDS }];
    state.cursor = { x: 2400, y: 500 };
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(false);
  });

  it('goose covered (occlusion): shown — on the display under the pointer, in the remembered corner', () => {
    const { desktop, state, facts } = setup({ displays: [MAIN, SIDE] });
    facts.windows = [{ onScreen: false, focused: false, bounds: GOOSE_BOUNDS }];
    state.cursor = { x: 2400, y: 500 };
    desktop.update(withPrefs(writing, { desktopPlace: { displayId: 1, corner: 'top-left' } }));
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(true);
    expect(state.bounds).toEqual({
      x: 1512 + GLANCE_MARGIN,
      y: GLANCE_MARGIN,
      width: 300,
      height: 180,
    });
  });

  it('goose uncovered again: the card goes', () => {
    const { desktop, state, facts } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(true);
    facts.windows = [{ onScreen: true, focused: false, bounds: GOOSE_BOUNDS }];
    desktop.refresh();
    expect(state.visible).toBe(false);
  });

  it('it grows while up: it keeps the display it showed on, though the pointer moved on', () => {
    const { desktop, state } = setup({ displays: [MAIN, SIDE] });
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.bounds.x).toBeLessThan(1512);
    state.cursor = { x: 2400, y: 500 };
    desktop.handle({ type: 'size', width: 300, height: 320 });
    expect(state.bounds.x).toBeLessThan(1512);
    expect(state.bounds.y + state.bounds.height).toBe(25 + 920 - GLANCE_MARGIN);
  });

  it('"whenever the engine works" with goose in view: never over goose — the next clear corner', () => {
    const { desktop, state, facts } = setup({ inFront: true });
    // goose's window covers MAIN's bottom-right quarter.
    facts.windows = [
      { onScreen: true, focused: true, bounds: { x: 756, y: 470, width: 756, height: 475 } },
    ];
    desktop.update(withPrefs(writing, { desktop: 'busy' }));
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(true);
    expect(state.bounds).toEqual({
      x: 1512 - GLANCE_MARGIN - 300,
      y: 25 + GLANCE_MARGIN,
      width: 300,
      height: 180,
    });
  });

  it('"whenever" and goose fills every display: it waits hidden; room again: it shows', () => {
    const { desktop, state, facts } = setup({ inFront: true });
    facts.windows = [{ onScreen: true, focused: true, bounds: MAIN.workArea }];
    desktop.update(withPrefs(writing, { desktop: 'busy' }));
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(false);
    facts.windows = [{ onScreen: true, focused: true, bounds: GOOSE_BOUNDS }];
    desktop.refresh();
    expect(state.visible).toBe(true);
  });

  it('"whenever", up, and a goose window comes into view under it: it moves off', () => {
    const { desktop, state, facts } = setup({ inFront: true });
    desktop.update(withPrefs(writing, { desktop: 'busy' }));
    desktop.handle({ type: 'size', width: 300, height: 180 });
    const first = state.bounds;
    facts.windows = [{ onScreen: true, focused: true, bounds: { ...first } }];
    desktop.refresh();
    expect(state.visible).toBe(true);
    expect(state.bounds).not.toEqual(first);
  });
});

describe('isGlancePipAction — what the window may ask', () => {
  it('accepts the actions it sends and nothing else', () => {
    expect(isGlancePipAction({ type: 'size', width: 300, height: 120 })).toBe(true);
    expect(isGlancePipAction({ type: 'turn-off' })).toBe(true);
    expect(isGlancePipAction({ type: 'size', width: 0, height: 120 })).toBe(false);
    expect(isGlancePipAction({ type: 'open-session', sessionId: '' })).toBe(false);
    expect(isGlancePipAction({ type: 'drag-move', screenX: Number.NaN, screenY: 1 })).toBe(false);
    expect(isGlancePipAction({ type: 'navigate', url: 'https://example.com' })).toBe(false);
  });
});
