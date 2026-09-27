import { describe, expect, it } from 'vitest';
import {
  cornerBounds,
  desktopGlanceVisible,
  dockRestorable,
  dockShown,
  glanceHasContent,
  glanceLive,
  gooseOnScreen,
  clearOfGoose,
  nearestCorner,
  placeGlance,
  snoozeAfter,
  workingDisplay,
  type GlanceDisplay,
  type GooseWindowFacts,
} from './engineGlanceRules';
import { INITIAL_SNAPSHOT } from './mlxEngineMonitor';
import { glancePush, runningSnapshot } from './engineGlance.fixtures';
import type { GlanceDesktopMode } from './engineGlance';
import {
  GENERATING_STATUS,
  IDLE_STATUS,
} from '../components/leanzero-swarm/mlxLiveStatus.fixtures';

const writing = glancePush(runningSnapshot(GENERATING_STATUS));
const idle = glancePush(runningSnapshot(IDLE_STATUS));
const off = glancePush({ ...INITIAL_SNAPSHOT, mode: 'off' });
const question = { sessionId: 's1', sessionName: 'Auth', question: 'Which branch?' };

describe('glanceLive — something a person would want to glance at', () => {
  it('the engine working is live; an idle engine is not', () => {
    expect(glanceLive(writing)).toBe(true);
    expect(glanceLive(idle)).toBe(false);
  });

  it('an idle engine between an agent’s model calls stays live while its turn runs its tools', () => {
    expect(glanceLive({ ...idle, sessions: { running: 1, needsYou: [] } })).toBe(true);
  });

  it('a turn in flight with NO engine (a cloud provider) is not the engine glance’s to show', () => {
    expect(glanceLive({ ...off, sessions: { running: 1, needsYou: [] } })).toBe(false);
  });

  it('a question waiting is live, engine or not', () => {
    expect(glanceLive({ ...off, sessions: { running: 0, needsYou: [question] } })).toBe(true);
    expect(glanceHasContent({ ...off, sessions: { running: 0, needsYou: [question] } })).toBe(true);
    expect(glanceHasContent(off)).toBe(false);
  });
});

describe('desktopGlanceVisible — the floating window on the desktop', () => {
  const withMode = (desktop: GlanceDesktopMode, push = writing) => ({
    ...push,
    prefs: { ...push.prefs, desktop },
  });

  it('default (away): shown while live and no goose window can be seen, never over goose itself', () => {
    expect(desktopGlanceVisible(writing, { gooseOnScreen: false, snoozed: false })).toBe(true);
    expect(desktopGlanceVisible(writing, { gooseOnScreen: true, snoozed: false })).toBe(false);
  });

  it('busy: shown whenever live, goose in sight or not (where is placeGlance’s to decide)', () => {
    expect(desktopGlanceVisible(withMode('busy'), { gooseOnScreen: true, snoozed: false })).toBe(
      true
    );
  });

  it('off: never', () => {
    expect(desktopGlanceVisible(withMode('off'), { gooseOnScreen: false, snoozed: false })).toBe(
      false
    );
  });

  it('nothing live: never, whatever the mode — an idle engine does not float over your work', () => {
    for (const mode of ['away', 'busy'] as const) {
      expect(
        desktopGlanceVisible(withMode(mode, idle), { gooseOnScreen: false, snoozed: false })
      ).toBe(false);
    }
  });

  it('closed: snoozed for this live spell, back on the next one', () => {
    expect(desktopGlanceVisible(writing, { gooseOnScreen: false, snoozed: true })).toBe(false);
    // Still live: the snooze holds.
    expect(snoozeAfter(true, writing)).toBe(true);
    // The spell ended: the snooze is spent, so the next spell shows it again.
    expect(snoozeAfter(true, idle)).toBe(false);
    expect(snoozeAfter(false, writing)).toBe(false);
  });
});

describe('the docked card — at the foot of the sidebar, or hidden by the person (Q-216..218)', () => {
  it('shown whenever there is something to show, idle included — no room is measured, nothing floats', () => {
    expect(dockShown(idle)).toBe(true);
    expect(dockShown(writing)).toBe(true);
    expect(dockRestorable(writing)).toBe(false);
  });

  it('hidden by the person: not shown, and the row that brings it back is offered', () => {
    const hidden = { ...writing, prefs: { ...writing.prefs, inApp: false } };
    expect(dockShown(hidden)).toBe(false);
    expect(dockRestorable(hidden)).toBe(true);
  });

  it('nothing to show: neither the card nor a row that would restore an empty card', () => {
    const hiddenOff = { ...off, prefs: { ...off.prefs, inApp: false } };
    expect(dockShown(off)).toBe(false);
    expect(dockRestorable(hiddenOff)).toBe(false);
  });

  it('a question with no engine still shows (the card is the question)', () => {
    const asked = { ...off, sessions: { running: 0, needsYou: [question] } };
    expect(dockShown(asked)).toBe(true);
  });
});

// The owner's four displays, as Electron reported them on 2026-09-27 (Q-226 probe).
const BUILT_IN: GlanceDisplay = { id: 1, workArea: { x: 0, y: 40, width: 2056, height: 1289 } };
const LG: GlanceDisplay = { id: 2, workArea: { x: -2560, y: -641, width: 2560, height: 1409 } };
const ARZOPA_L: GlanceDisplay = { id: 4, workArea: { x: 0, y: -1249, width: 2048, height: 1249 } };
const ARZOPA_R: GlanceDisplay = {
  id: 5,
  workArea: { x: 2048, y: -1249, width: 2048, height: 1249 },
};
const DISPLAYS = [BUILT_IN, LG, ARZOPA_L, ARZOPA_R];
const gooseWin = (over: Partial<GooseWindowFacts> = {}): GooseWindowFacts => ({
  onScreen: true,
  focused: false,
  bounds: { x: 100, y: 100, width: 940, height: 800 },
  ...over,
});

describe('gooseOnScreen — the fact the "in the background" desktop window hangs on (Q-226)', () => {
  it('the owner’s screenshot: goose in plain view on one display, another app focused on another — goose is SEEN', () => {
    expect(gooseOnScreen('darwin', [gooseWin({ focused: false })])).toBe(true);
  });

  it('goose focused, or its own folder panel up with no window focused (Q-217) — seen', () => {
    expect(gooseOnScreen('darwin', [gooseWin({ focused: true })])).toBe(true);
  });

  it('covered, on a Space not showing, minimized or hidden — every window out of sight: NOT seen', () => {
    expect(gooseOnScreen('darwin', [gooseWin({ onScreen: false })])).toBe(false);
    expect(gooseOnScreen('darwin', [])).toBe(false);
  });

  it('one window out of sight, another in view: seen', () => {
    expect(gooseOnScreen('darwin', [gooseWin({ onScreen: false }), gooseWin()])).toBe(true);
  });

  it('elsewhere (no occlusion reported): a focused goose window is the fact, as before', () => {
    expect(gooseOnScreen('linux', [gooseWin({ focused: true })])).toBe(true);
    expect(gooseOnScreen('win32', [gooseWin({ focused: false })])).toBe(false);
  });
});

describe('workingDisplay — where the person is working', () => {
  it('a focused goose window’s display', () => {
    const onLg = gooseWin({
      focused: true,
      bounds: { x: -2000, y: -300, width: 900, height: 700 },
    });
    expect(workingDisplay(DISPLAYS, [onLg], { x: 500, y: 500 }).id).toBe(2);
  });

  it('goose out of sight (another app focused): the pointer’s display', () => {
    const covered = gooseWin({ onScreen: false, focused: false });
    expect(workingDisplay(DISPLAYS, [covered], { x: 3000, y: -600 }).id).toBe(5);
    expect(workingDisplay(DISPLAYS, [], { x: 500, y: -600 }).id).toBe(4);
  });

  it('the pointer in a menu bar (outside every work area): the nearest display', () => {
    expect(workingDisplay(DISPLAYS, [], { x: 800, y: 20 }).id).toBe(1);
  });
});

describe('placeGlance — on the working display, never over goose (Q-226)', () => {
  const size = { width: 300, height: 180 };
  const base = { displays: DISPLAYS, defaultCorner: 'bottom-right' as const, size, margin: 16 };

  it('goose out of sight: the working display, in the remembered corner even though it was dropped on another display', () => {
    const p = placeGlance({
      ...base,
      working: ARZOPA_R,
      remembered: { displayId: 2, corner: 'top-left' },
      windows: [gooseWin({ onScreen: false })],
    });
    expect(p).toEqual({
      displayId: 5,
      corner: 'top-left',
      bounds: { x: 2048 + 16, y: -1249 + 16, width: 300, height: 180 },
    });
  });

  it('nothing remembered: the default corner', () => {
    const p = placeGlance({ ...base, working: BUILT_IN, remembered: null, windows: [] });
    expect(p?.corner).toBe('bottom-right');
    expect(p?.displayId).toBe(1);
  });

  it('a goose window in view under that corner: the next free corner of the same display', () => {
    const bottomRightHalf = gooseWin({ bounds: { x: 1028, y: 684, width: 1028, height: 645 } });
    const p = placeGlance({
      ...base,
      working: BUILT_IN,
      remembered: null,
      windows: [bottomRightHalf],
    });
    expect(p?.displayId).toBe(1);
    expect(p?.corner).toBe('top-right');
  });

  it('goose fills the working display: another display — the remembered one first', () => {
    const full = gooseWin({ bounds: BUILT_IN.workArea });
    const p = placeGlance({
      ...base,
      working: BUILT_IN,
      remembered: { displayId: 5, corner: 'bottom-left' },
      windows: [full],
    });
    expect(p).toMatchObject({ displayId: 5, corner: 'bottom-left' });
  });

  it('goose in view over every display: it does not show at all', () => {
    const windows = DISPLAYS.map((d) => gooseWin({ bounds: d.workArea }));
    expect(placeGlance({ ...base, working: BUILT_IN, remembered: null, windows })).toBeNull();
  });

  it('a goose window out of sight never pushes it anywhere', () => {
    const coveredFull = gooseWin({ onScreen: false, bounds: BUILT_IN.workArea });
    const p = placeGlance({ ...base, working: BUILT_IN, remembered: null, windows: [coveredFull] });
    expect(p).toMatchObject({ displayId: 1, corner: 'bottom-right' });
  });

  it('clearOfGoose: only windows that can be seen count', () => {
    const r = { x: 0, y: 0, width: 100, height: 100 };
    expect(clearOfGoose(r, [gooseWin({ bounds: { x: 50, y: 50, width: 100, height: 100 } })])).toBe(
      false
    );
    expect(
      clearOfGoose(r, [
        gooseWin({ onScreen: false, bounds: { x: 50, y: 50, width: 100, height: 100 } }),
      ])
    ).toBe(true);
    // Touching edges is not covering.
    expect(clearOfGoose(r, [gooseWin({ bounds: { x: 100, y: 0, width: 100, height: 100 } })])).toBe(
      true
    );
  });
});

describe('the geometry', () => {
  const area = { x: 0, y: 25, width: 1512, height: 920 };

  it('nearestCorner: the quadrant of the window’s centre', () => {
    expect(nearestCorner({ x: 10, y: 40, width: 300, height: 150 }, area)).toBe('top-left');
    expect(nearestCorner({ x: 1200, y: 40, width: 300, height: 150 }, area)).toBe('top-right');
    expect(nearestCorner({ x: 10, y: 800, width: 300, height: 150 }, area)).toBe('bottom-left');
    expect(nearestCorner({ x: 1000, y: 700, width: 300, height: 150 }, area)).toBe('bottom-right');
  });

  it('cornerBounds: inset by the margin from the work area (below the menu bar), never off it', () => {
    expect(cornerBounds('bottom-right', { width: 300, height: 150 }, area, 16)).toEqual({
      x: 1512 - 16 - 300,
      y: 25 + 920 - 16 - 150,
      width: 300,
      height: 150,
    });
    expect(cornerBounds('top-left', { width: 300, height: 150 }, area, 16)).toEqual({
      x: 16,
      y: 41,
      width: 300,
      height: 150,
    });
    // A card taller than the display is clamped to it.
    expect(cornerBounds('top-left', { width: 300, height: 5000 }, area, 16).height).toBe(888);
  });
});
