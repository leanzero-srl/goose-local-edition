import { describe, expect, it, vi } from 'vitest';
import {
  EngineGlanceDesktop,
  GLANCE_MARGIN,
  isGlancePipAction,
  type GlanceDisplay,
  type GlanceWindowPort,
} from './engineGlanceDesktop';
import { ENGINE_GLANCE_CHANNEL, type GlancePrefs, type GlancePush } from './utils/engineGlance';
import type { Rect } from './utils/engineGlanceRules';
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
    primaryDisplay: () => state.displays[0],
  };
  return { port, state };
}

function setup(opts: { inFront?: boolean; displays?: GlanceDisplay[] } = {}) {
  const { port, state } = fakePort(opts.displays);
  const facts = { inFront: opts.inFront ?? false };
  const saved: GlancePrefs[] = [];
  const openEngine = vi.fn();
  const openSession = vi.fn();
  const desktop = new EngineGlanceDesktop({
    port,
    appInFront: () => facts.inFront,
    savePrefs: (p) => saved.push(p),
    openEngine,
    openSession,
  });
  return { desktop, state, facts, saved, openEngine, openSession };
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
    desktop.handle({ type: 'drag-start', screenX: 10, screenY: 10 });
    desktop.handle({ type: 'drag-end' });
    expect(saved).toHaveLength(0);
  });

  it('a remembered display that is gone: the same corner of the primary display', () => {
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

describe('isGlancePipAction — what the window may ask', () => {
  it('accepts the actions it sends and nothing else', () => {
    expect(isGlancePipAction({ type: 'size', width: 300, height: 120 })).toBe(true);
    expect(isGlancePipAction({ type: 'size', width: 0, height: 120 })).toBe(false);
    expect(isGlancePipAction({ type: 'open-session', sessionId: '' })).toBe(false);
    expect(isGlancePipAction({ type: 'drag-move', screenX: Number.NaN, screenY: 1 })).toBe(false);
    expect(isGlancePipAction({ type: 'navigate', url: 'https://example.com' })).toBe(false);
  });
});
