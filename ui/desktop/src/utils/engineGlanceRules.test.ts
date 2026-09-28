import { describe, expect, it } from 'vitest';
import {
  GLANCE_COVERED_SHARE,
  cornerBounds,
  coverageWanted,
  desktopGlanceVisible,
  dockRestorable,
  dockShown,
  glanceHasContent,
  glanceLive,
  gooseOnScreen,
  clearOfGoose,
  nearestCorner,
  placeGlance,
  visibleShare,
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
    expect(desktopGlanceVisible(writing, { gooseOnScreen: false, dismissed: false })).toBe(true);
    expect(desktopGlanceVisible(writing, { gooseOnScreen: true, dismissed: false })).toBe(false);
  });

  it('busy: shown whenever live, goose in sight or not (where is placeGlance’s to decide)', () => {
    expect(desktopGlanceVisible(withMode('busy'), { gooseOnScreen: true, dismissed: false })).toBe(
      true
    );
  });

  it('off: never', () => {
    expect(desktopGlanceVisible(withMode('off'), { gooseOnScreen: false, dismissed: false })).toBe(
      false
    );
  });

  it('nothing live: never, whatever the mode — an idle engine does not float over your work', () => {
    for (const mode of ['away', 'busy'] as const) {
      expect(
        desktopGlanceVisible(withMode(mode, idle), { gooseOnScreen: false, dismissed: false })
      ).toBe(false);
    }
  });

  it('Q-426: closed for the session — never shown, whatever the mode or the work', () => {
    for (const mode of ['away', 'busy'] as const) {
      expect(desktopGlanceVisible(withMode(mode), { gooseOnScreen: false, dismissed: true })).toBe(
        false
      );
    }
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
  visibleShare: null,
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

// Q-313, measured by the 3.0.68 critic: goose's window on the built-in display, Windows App's
// 2048×1280 window over it leaving an 8 px column and a 9 px row — an L of 28,744 px² (1.08%).
const Q313_GOOSE = { x: 0, y: 40, width: 2056, height: 1289 };
const Q313_WINDOWS_APP = { x: 8, y: 49, width: 2048, height: 1280 };

describe('Q-313 — a goose window covered but for a sliver counts as covered', () => {
  it('visibleShare: the critic’s L is 1.08% of the window — under the covered share', () => {
    const share = visibleShare(Q313_GOOSE, [Q313_WINDOWS_APP]);
    expect(share).toBeCloseTo(28_744 / (2056 * 1289), 10);
    expect(share).toBeLessThan(GLANCE_COVERED_SHARE);
  });

  it('visibleShare: overlapping covers are counted once, rects off the window not at all', () => {
    const t = { x: 0, y: 0, width: 100, height: 100 };
    expect(visibleShare(t, [])).toBe(1);
    expect(
      visibleShare(t, [
        { x: 0, y: 0, width: 60, height: 100 },
        { x: 40, y: 0, width: 60, height: 100 },
      ])
    ).toBe(0);
    expect(
      visibleShare(t, [
        { x: 0, y: 0, width: 50, height: 100 },
        { x: 25, y: 0, width: 50, height: 100 },
      ])
    ).toBe(0.25);
    expect(visibleShare(t, [{ x: 200, y: 200, width: 50, height: 50 }])).toBe(1);
  });

  it('goose behind another app with a sliver left: NOT seen — the desktop window shows', () => {
    const sliver = gooseWin({
      bounds: Q313_GOOSE,
      visibleShare: visibleShare(Q313_GOOSE, [Q313_WINDOWS_APP]),
    });
    expect(gooseOnScreen('darwin', [sliver])).toBe(false);
    expect(
      desktopGlanceVisible(writing, {
        gooseOnScreen: gooseOnScreen('darwin', [sliver]),
        dismissed: false,
      })
    ).toBe(true);
  });

  it('half covered, or not read: seen, as before (Q-226)', () => {
    expect(gooseOnScreen('darwin', [gooseWin({ visibleShare: 0.5 })])).toBe(true);
    expect(gooseOnScreen('darwin', [gooseWin({ visibleShare: GLANCE_COVERED_SHARE })])).toBe(true);
    expect(gooseOnScreen('darwin', [gooseWin({ visibleShare: null })])).toBe(true);
  });

  it('goose in front: a share read while it was behind never counts', () => {
    expect(gooseOnScreen('darwin', [gooseWin({ focused: true, visibleShare: 0.01 })])).toBe(true);
    expect(
      gooseOnScreen('darwin', [
        gooseWin({ visibleShare: 0.01 }),
        gooseWin({ focused: true, onScreen: true }),
      ])
    ).toBe(true);
  });

  it('placement: a window covered but for a sliver never pushes the card off its display', () => {
    const sliver = gooseWin({ bounds: BUILT_IN.workArea, visibleShare: 0.0108 });
    const p = placeGlance({
      displays: DISPLAYS,
      working: BUILT_IN,
      remembered: null,
      defaultCorner: 'bottom-right',
      windows: [sliver],
      size: { width: 300, height: 180 },
      margin: 16,
    });
    expect(p).toMatchObject({ displayId: 1, corner: 'bottom-right' });
    expect(clearOfGoose(p!.bounds, [sliver])).toBe(true);
  });

  it('coverageWanted: macOS, live, goose behind another app and still reported on screen — nothing else', () => {
    const behind = [gooseWin()];
    expect(coverageWanted('darwin', writing, false, behind)).toBe(true);
    expect(coverageWanted('linux', writing, false, behind)).toBe(false);
    expect(coverageWanted('darwin', null, false, behind)).toBe(false);
    expect(coverageWanted('darwin', idle, false, behind)).toBe(false);
    expect(coverageWanted('darwin', writing, true, behind)).toBe(false);
    expect(
      coverageWanted(
        'darwin',
        { ...writing, prefs: { ...writing.prefs, desktop: 'off' } },
        false,
        behind
      )
    ).toBe(false);
    expect(coverageWanted('darwin', writing, false, [gooseWin({ focused: true })])).toBe(false);
    expect(coverageWanted('darwin', writing, false, [gooseWin({ onScreen: false })])).toBe(false);
    expect(
      coverageWanted(
        'darwin',
        { ...writing, prefs: { ...writing.prefs, desktop: 'busy' } },
        false,
        behind
      )
    ).toBe(true);
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
