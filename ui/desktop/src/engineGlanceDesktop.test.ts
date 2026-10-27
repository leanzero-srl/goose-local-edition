import { describe, expect, it, vi } from 'vitest';
import {
  EngineGlanceDesktop,
  GLANCE_MARGIN,
  RAISE_PAIR_MS,
  isGlancePipAction,
  trayOffersGlanceBack,
  type GlanceDisplay,
  type GlanceWindowPort,
} from './engineGlanceDesktop';
import { ENGINE_GLANCE_CHANNEL, type GlancePrefs, type GlancePush } from './utils/engineGlance';
import type { GooseWindowFacts, Rect } from './utils/engineGlanceRules';
import { glancePush, runningSnapshot } from './utils/engineGlance.fixtures';
import { GENERATING_STATUS, IDLE_STATUS } from './components/leanzero-swarm/mlxLiveStatus.fixtures';
import { INITIAL_SNAPSHOT } from './utils/mlxEngineMonitor';
import { toMlxDistributedReport } from './utils/mlxDistributedReport';
import { FLASH_SERVING, HOSTING_RANK_1 } from './components/leanzero-swarm/mlxDistributed.fixtures';

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
  // The goose window in front hears "closed for the session" only while goose IS in front (main's rule).
  const told: string[] = [];
  const dismissedChanged = vi.fn();
  const coverage = { measure: vi.fn(), forget: vi.fn() };
  // A clock the test moves, and the timers main would run on it.
  const clock = { now: 1_000_000 };
  const timers: { at: number; fn: () => void }[] = [];
  const reverted: string[] = [];
  const advance = (ms: number) => {
    clock.now += ms;
    for (const t of timers.splice(0).sort((a, b) => a.at - b.at)) {
      if (t.at <= clock.now) t.fn();
      else timers.push(t);
    }
  };
  const desktop = new EngineGlanceDesktop({
    port,
    platform: 'darwin',
    gooseWindows: () =>
      facts.windows ?? [
        {
          onScreen: facts.inFront,
          focused: facts.inFront,
          bounds: GOOSE_BOUNDS,
          visibleShare: null,
        },
      ],
    savePrefs: (p) => saved.push(p),
    coverage,
    openEngine,
    openSession,
    tellDismissed: () => {
      if (!facts.inFront) return false;
      told.push('dismissed');
      return true;
    },
    dismissedChanged,
    now: () => clock.now,
    later: (fn, ms) => timers.push({ at: clock.now + ms, fn }),
    revertRaise: (id, kind) => reverted.push(`${id}:${kind}`),
  });
  return {
    clock,
    advance,
    reverted,
    desktop,
    state,
    facts,
    saved,
    told,
    coverage,
    openEngine,
    openSession,
    dismissedChanged,
  };
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

  it("Q-426: closed — the same work's next busy spell does NOT bring it back", () => {
    const { desktop, state } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.handle({ type: 'close' });
    expect(state.visible).toBe(false);
    // Nothing kept alive for it, and nothing made again while it is closed.
    expect(state.exists).toBe(false);
    desktop.update(writing);
    desktop.update(idle);
    desktop.update(writing);
    desktop.refresh();
    expect(state.exists).toBe(false);
    expect(state.calls.filter((c) => c === 'ensure')).toHaveLength(1);
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

describe('EngineGlanceDesktop — Q-426: the X closes it for the session, and only the person brings it back', () => {
  it('the X: hidden and destroyed at once, no goose window opened, no setting written', () => {
    const { desktop, state, saved, openEngine, openSession, dismissedChanged } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(true);
    const before = saved.length;
    desktop.handle({ type: 'close' });
    expect(state.visible).toBe(false);
    expect(state.exists).toBe(false);
    expect(desktop.isDismissed()).toBe(true);
    expect(dismissedChanged).toHaveBeenCalledOnce();
    expect(openEngine).not.toHaveBeenCalled();
    expect(openSession).not.toHaveBeenCalled();
    // Session state, never a setting: the next launch reads the stored mode as it was.
    expect(saved).toHaveLength(before);
  });

  it('"whenever the engine works" and goose in front: closed stays closed too', () => {
    const { desktop, state } = setup({ inFront: true });
    desktop.update(withPrefs(writing, { desktop: 'busy' }));
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(true);
    desktop.handle({ type: 'close' });
    desktop.update(withPrefs(writing, { desktop: 'busy' }));
    expect(state.exists).toBe(false);
  });

  it('brought back (the menu-bar item, Settings › App): it shows again by the ordinary rules', () => {
    const { desktop, state } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.handle({ type: 'close' });
    expect(desktop.showAgain()).toBe(true);
    expect(desktop.isDismissed()).toBe(false);
    // Made again and shown inactive, as always — never focused, never activating goose.
    expect(state.visible).toBe(true);
    expect(state.calls.slice(-4)).toEqual(['hide', 'destroy', 'ensure', 'showInactive']);
  });

  it('negative control: bringing back a window nobody closed changes nothing', () => {
    const { desktop, state } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    const before = [...state.calls];
    expect(desktop.showAgain()).toBe(false);
    expect(state.calls).toEqual(before);
  });

  it('goose in the background at the click: said once a goose window is in front — once a session', () => {
    const { desktop, facts, told } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.handle({ type: 'close' });
    expect(told).toEqual([]);
    // Every snapshot re-decides; the person is still in the other app.
    desktop.refresh();
    expect(told).toEqual([]);
    facts.inFront = true;
    desktop.refresh();
    expect(told).toEqual(['dismissed']);
    desktop.refresh();
    expect(told).toEqual(['dismissed']);
    // Brought back and closed again: the person already knows the way back.
    desktop.showAgain();
    facts.inFront = false;
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.handle({ type: 'close' });
    facts.inFront = true;
    desktop.refresh();
    expect(told).toEqual(['dismissed']);
  });

  it('brought back before any goose window could say it: nothing is said later', () => {
    const { desktop, facts, told } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.handle({ type: 'close' });
    desktop.showAgain();
    facts.inFront = true;
    desktop.refresh();
    expect(told).toEqual([]);
  });

  it('the menu bar offers it back only while it is closed and a floating mode is on', () => {
    expect(trayOffersGlanceBack(true, writing.prefs)).toBe(true);
    expect(trayOffersGlanceBack(false, writing.prefs)).toBe(false);
    expect(trayOffersGlanceBack(true, { ...writing.prefs, desktop: 'off' })).toBe(false);
  });

  it('turned off from Settings › App: destroyed, and nothing is said', () => {
    const { desktop, state, facts, told } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.update(withPrefs(writing, { desktop: 'off' }));
    expect(state.exists).toBe(false);
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
    facts.windows = [{ onScreen: true, focused: false, bounds: GOOSE_BOUNDS, visibleShare: null }];
    state.cursor = { x: 2400, y: 500 };
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(false);
  });

  it('goose covered (occlusion): shown — on the display under the pointer, in the remembered corner', () => {
    const { desktop, state, facts } = setup({ displays: [MAIN, SIDE] });
    facts.windows = [{ onScreen: false, focused: false, bounds: GOOSE_BOUNDS, visibleShare: null }];
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

  it('Q-313: goose behind another app with only a sliver showing (no occlusion event) — shown', () => {
    const { desktop, state, facts, coverage } = setup();
    facts.windows = [{ onScreen: true, focused: false, bounds: MAIN.workArea, visibleShare: null }];
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(false);
    expect(coverage.measure).toHaveBeenCalled();
    // The window server's read lands: 1.08% of goose left uncovered.
    facts.windows = [
      { onScreen: true, focused: false, bounds: MAIN.workArea, visibleShare: 0.0108 },
    ];
    desktop.refresh();
    expect(state.visible).toBe(true);
    expect(state.bounds).toEqual({
      x: 1512 - GLANCE_MARGIN - 300,
      y: 25 + 920 - GLANCE_MARGIN - 180,
      width: 300,
      height: 180,
    });
    // goose comes to the front: hidden, and the list read while it was behind is dropped.
    facts.windows = [
      { onScreen: true, focused: true, bounds: MAIN.workArea, visibleShare: 0.0108 },
    ];
    desktop.refresh();
    expect(state.visible).toBe(false);
    expect(coverage.forget).toHaveBeenCalled();
  });

  it('goose uncovered again: the card goes', () => {
    const { desktop, state, facts } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(true);
    facts.windows = [{ onScreen: true, focused: false, bounds: GOOSE_BOUNDS, visibleShare: null }];
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
      {
        onScreen: true,
        focused: true,
        bounds: { x: 756, y: 470, width: 756, height: 475 },
        visibleShare: null,
      },
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
    facts.windows = [{ onScreen: true, focused: true, bounds: MAIN.workArea, visibleShare: null }];
    desktop.update(withPrefs(writing, { desktop: 'busy' }));
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(false);
    facts.windows = [{ onScreen: true, focused: true, bounds: GOOSE_BOUNDS, visibleShare: null }];
    desktop.refresh();
    expect(state.visible).toBe(true);
  });

  it('"whenever", up, and a goose window comes into view under it: it moves off', () => {
    const { desktop, state, facts } = setup({ inFront: true });
    desktop.update(withPrefs(writing, { desktop: 'busy' }));
    desktop.handle({ type: 'size', width: 300, height: 180 });
    const first = state.bounds;
    facts.windows = [{ onScreen: true, focused: true, bounds: { ...first }, visibleShare: null }];
    desktop.refresh();
    expect(state.visible).toBe(true);
    expect(state.bounds).not.toEqual(first);
  });
});

// Q-229: installing 3.0.63, the quit closed the goose window and ended goosed, and the app then lived
// on with two floating windows until SIGKILL. Electron's quit finishes only when every window is
// closed, and each close/blur and each engine snapshot re-decided the floating window — which, with
// goose out of sight and the engine live, MADE it again.
describe('EngineGlanceDesktop — Q-229: the quit is never held open by the floating window', () => {
  it('the quit begins with it up: destroyed at once', () => {
    const { desktop, state } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(state.visible).toBe(true);
    desktop.suspendForQuit();
    expect(state.exists).toBe(false);
    expect(state.calls[state.calls.length - 1]).toBe('destroy');
  });

  it('while quitting, the goose window closing and every later snapshot make nothing', () => {
    const { desktop, state, facts } = setup({ inFront: true });
    desktop.update(writing);
    desktop.suspendForQuit();
    const before = state.calls.length;
    // The goose window closes (out of sight), the engine keeps writing, a drag was mid-way.
    facts.inFront = false;
    facts.windows = [];
    desktop.refresh();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.displaysChanged();
    expect(state.calls.slice(before)).toEqual([]);
    expect(state.exists).toBe(false);
  });

  it('the quit is refused (a live run’s close guard): it comes back by the same rules', () => {
    const { desktop, state } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    desktop.suspendForQuit();
    desktop.resumeAfterRefusedQuit();
    expect(state.visible).toBe(true);
    expect(state.calls.slice(-3)).toEqual(['destroy', 'ensure', 'showInactive']);
  });

  it('a refused close that was not part of a quit changes nothing', () => {
    const { desktop, state } = setup();
    desktop.update(writing);
    desktop.handle({ type: 'size', width: 300, height: 180 });
    const before = [...state.calls];
    desktop.resumeAfterRefusedQuit();
    expect(state.calls).toEqual(before);
  });
});

describe('isGlancePipAction — what the window may ask', () => {
  it('accepts the actions it sends and nothing else', () => {
    expect(isGlancePipAction({ type: 'size', width: 300, height: 120 })).toBe(true);
    expect(isGlancePipAction({ type: 'close' })).toBe(true);
    // Q-426: the X no longer turns the setting off from the window; main refuses the old ask.
    expect(isGlancePipAction({ type: 'turn-off' })).toBe(false);
    expect(isGlancePipAction({ type: 'size', width: 0, height: 120 })).toBe(false);
    expect(isGlancePipAction({ type: 'open-session', sessionId: '' })).toBe(false);
    expect(isGlancePipAction({ type: 'drag-move', screenX: Number.NaN, screenY: 1 })).toBe(false);
    expect(isGlancePipAction({ type: 'navigate', url: 'https://example.com' })).toBe(false);
  });
});

/** Shown in the bottom-right of MAIN (setup's 300×180), with the pointer resting on it. */
function upWithPointerOnIt(opts: Parameters<typeof setup>[0] = {}, push: GlancePush = writing) {
  const t = setup(opts);
  t.desktop.update(push);
  t.desktop.handle({ type: 'size', width: 300, height: 180 });
  expect(t.state.visible).toBe(true);
  t.state.cursor = { x: t.state.bounds.x + 280, y: t.state.bounds.y + 12 };
  return t;
}

describe('EngineGlanceDesktop — owner 2026-10-27: a REAL click on X or minimize never opens goose', () => {
  // A real click activates goose (Electron's "panel" is not an NSPanel), AppKit makes goose's window
  // key and brings it forward, and main hears that focus BEFORE the release reaches the X.
  it('the press raises goose: the raise is put back, the window stays up, and the X then closes it', () => {
    const t = upWithPointerOnIt();
    t.desktop.handle({ type: 'press' });
    t.facts.inFront = true;
    t.desktop.gooseRaised(1, 'focus');
    t.desktop.refresh();
    expect(t.reverted).toEqual(['1:focus']);
    expect(t.state.visible).toBe(true);
    t.facts.inFront = false;
    t.desktop.handle({ type: 'close' });
    expect(t.state.exists).toBe(false);
    expect(t.openEngine).not.toHaveBeenCalled();
    // Goose is behind other apps again: the blur that follows never brings it back.
    t.advance(5_000);
    t.desktop.refresh();
    t.desktop.update(writing);
    expect(t.state.exists).toBe(false);
  });

  it('the raise lands before the press reaches main: paired all the same, never hidden in between', () => {
    const t = upWithPointerOnIt();
    t.facts.inFront = true;
    t.desktop.gooseRaised(1, 'focus');
    t.desktop.refresh();
    expect(t.state.visible).toBe(true);
    t.advance(120);
    t.desktop.handle({ type: 'press' });
    expect(t.reverted).toEqual(['1:focus']);
    t.facts.inFront = false;
    t.desktop.handle({ type: 'collapse', collapsed: true });
    expect(t.state.visible).toBe(true);
    expect(t.saved[t.saved.length - 1]?.desktopCollapsed).toBe(true);
  });

  it('a minimized goose window the press restored is minimized again', () => {
    const t = upWithPointerOnIt();
    t.desktop.handle({ type: 'press' });
    t.desktop.gooseRaised(3, 'restore');
    expect(t.reverted).toEqual(['3:restore']);
  });

  it('Cmd-Tab to goose with the pointer resting on the card (no press): not put back, and it hides', () => {
    const t = upWithPointerOnIt();
    t.facts.inFront = true;
    t.desktop.gooseRaised(1, 'focus');
    t.desktop.refresh();
    t.advance(RAISE_PAIR_MS + 5);
    expect(t.reverted).toEqual([]);
    expect(t.state.visible).toBe(false);
  });

  it('the Open control raises goose on purpose: never put back', () => {
    const t = upWithPointerOnIt();
    t.desktop.handle({ type: 'press' });
    t.desktop.handle({ type: 'open-engine' });
    t.facts.inFront = true;
    t.desktop.gooseRaised(1, 'focus');
    expect(t.reverted).toEqual([]);
    expect(t.openEngine).toHaveBeenCalledOnce();
  });

  it("goose came forward with the pointer elsewhere: not the click's doing", () => {
    const t = upWithPointerOnIt();
    t.desktop.handle({ type: 'press' });
    t.state.cursor = { x: 10, y: 400 };
    t.desktop.gooseRaised(1, 'focus');
    expect(t.reverted).toEqual([]);
  });

  it('a goose window already in view before the click ("whenever the engine works") stays where it is', () => {
    const t = setup({ inFront: false });
    t.facts.windows = [
      { id: 5, onScreen: true, focused: false, bounds: GOOSE_BOUNDS, visibleShare: null },
    ];
    t.desktop.update(withPrefs(writing, { desktop: 'busy' }));
    t.desktop.handle({ type: 'size', width: 300, height: 180 });
    t.state.cursor = { x: t.state.bounds.x + 5, y: t.state.bounds.y + 5 };
    t.desktop.handle({ type: 'press' });
    t.desktop.gooseRaised(5, 'focus');
    expect(t.reverted).toEqual([]);
  });
});

const turn = (id: string, startedAt: string) => ({
  running: 1,
  needsYou: [],
  runningKeys: [`${id}@${startedAt}`],
});

describe('EngineGlanceDesktop — owner 2026-10-27: a close holds until a NEW process starts', () => {
  it("closed during a turn: blur, focus-out and the same turn's busy spells never bring it back", () => {
    const t = setup();
    const during = { ...writing, sessions: turn('s1', '2026-10-27T10:00:00Z') };
    t.desktop.update(during);
    t.desktop.handle({ type: 'size', width: 300, height: 180 });
    t.desktop.handle({ type: 'close' });
    for (let i = 0; i < 5; i++) {
      t.advance(60_000);
      t.desktop.update({ ...idle, sessions: during.sessions });
      t.desktop.refresh();
      t.desktop.update(during);
    }
    expect(t.state.exists).toBe(false);
    expect(t.desktop.isDismissed()).toBe(true);
  });

  it('a NEW turn starts (another chat, or the next run): it comes back, and Settings is told', () => {
    const t = setup();
    t.desktop.update({ ...writing, sessions: turn('s1', '2026-10-27T10:00:00Z') });
    t.desktop.handle({ type: 'size', width: 300, height: 180 });
    t.desktop.handle({ type: 'close' });
    t.dismissedChanged.mockClear();
    t.desktop.update({ ...writing, sessions: turn('s2', '2026-10-27T10:05:00Z') });
    expect(t.desktop.isDismissed()).toBe(false);
    expect(t.state.visible).toBe(true);
    expect(t.dismissedChanged).toHaveBeenCalledOnce();
  });

  it('a split starts after the close: a new process, it comes back', () => {
    const t = setup();
    t.desktop.update({ ...writing, sessions: turn('s1', '2026-10-27T10:00:00Z') });
    t.desktop.handle({ type: 'size', width: 300, height: 180 });
    t.desktop.handle({ type: 'close' });
    const split = glancePush(INITIAL_SNAPSHOT, {
      distributed: { report: toMlxDistributedReport(FLASH_SERVING), ageMs: 0 },
    });
    t.desktop.update({ ...split, sessions: turn('s1', '2026-10-27T10:00:00Z') });
    expect(t.desktop.isDismissed()).toBe(false);
  });

  it('negative control: the same turn and the same engine, minutes later, keep it closed', () => {
    const t = setup();
    const during = { ...writing, sessions: turn('s1', '2026-10-27T10:00:00Z') };
    t.desktop.update(during);
    t.desktop.handle({ type: 'size', width: 300, height: 180 });
    t.desktop.handle({ type: 'close' });
    t.advance(10 * 60_000);
    t.desktop.update(during);
    expect(t.desktop.isDismissed()).toBe(true);
  });
});

describe('EngineGlanceDesktop — owner 2026-10-27: the Mac serving a rank shows it too', () => {
  it("a rank of another Mac's split, idle between requests, floats on this Mac", () => {
    const t = setup();
    const peer = glancePush(INITIAL_SNAPSHOT, {
      distributed: { report: toMlxDistributedReport(HOSTING_RANK_1), ageMs: 0 },
    });
    expect(peer.engine.busy).toBe(false);
    t.desktop.update(peer);
    t.desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(t.state.visible).toBe(true);
  });

  it('a rank launched here over ssh (no goosed report): from the process list, with its layers', () => {
    const t = setup();
    const peer = glancePush(INITIAL_SNAPSHOT, {
      peerRanks: [
        {
          pid: 77,
          rank: 1,
          size: 2,
          modelId: 'qwen4-flash',
          backend: 'jaccl',
          requester: 'workhorse',
          layers: { first: 17, last: 47 },
        },
      ],
    });
    expect(peer.engine.engine).toMatchObject({
      mode: 'hosting',
      requester: 'workhorse',
      layers: { first: 17, last: 47 },
    });
    t.desktop.update(peer);
    t.desktop.handle({ type: 'size', width: 300, height: 180 });
    expect(t.state.visible).toBe(true);
  });
});
