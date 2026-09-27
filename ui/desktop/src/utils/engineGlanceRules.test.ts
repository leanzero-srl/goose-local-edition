import { describe, expect, it } from 'vitest';
import {
  cornerBounds,
  desktopGlanceVisible,
  dockRestorable,
  dockShown,
  glanceHasContent,
  glanceLive,
  gooseInFront,
  nearestCorner,
  snoozeAfter,
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

  it('default (away): shown while live and goose is in the background, never over goose itself', () => {
    expect(desktopGlanceVisible(writing, { appInFront: false, snoozed: false })).toBe(true);
    expect(desktopGlanceVisible(writing, { appInFront: true, snoozed: false })).toBe(false);
  });

  it('busy: shown whenever live, goose in front or not', () => {
    expect(desktopGlanceVisible(withMode('busy'), { appInFront: true, snoozed: false })).toBe(true);
  });

  it('off: never', () => {
    expect(desktopGlanceVisible(withMode('off'), { appInFront: false, snoozed: false })).toBe(
      false
    );
  });

  it('nothing live: never, whatever the mode — an idle engine does not float over your work', () => {
    for (const mode of ['away', 'busy'] as const) {
      expect(
        desktopGlanceVisible(withMode(mode, idle), { appInFront: false, snoozed: false })
      ).toBe(false);
    }
  });

  it('closed: snoozed for this live spell, back on the next one', () => {
    expect(desktopGlanceVisible(writing, { appInFront: false, snoozed: true })).toBe(false);
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

describe('gooseInFront — the fact the "in the background" desktop window hangs on', () => {
  const onScreen = { visible: true, minimized: false };
  it('macOS: goose active with a window on screen is in front, even with NO window focused (its own folder panel open)', () => {
    expect(gooseInFront('darwin', true, false, [onScreen])).toBe(true);
  });

  it('macOS: another app in front — goose is in the background', () => {
    expect(gooseInFront('darwin', false, false, [onScreen])).toBe(false);
  });

  it('macOS: goose still the active app but every window minimized or hidden — the owner’s original ask', () => {
    expect(gooseInFront('darwin', true, false, [{ visible: true, minimized: true }])).toBe(false);
    expect(gooseInFront('darwin', true, false, [{ visible: false, minimized: false }])).toBe(false);
    expect(gooseInFront('darwin', true, false, [])).toBe(false);
  });

  it('elsewhere: a focused goose window is the fact', () => {
    expect(gooseInFront('linux', false, true, [])).toBe(true);
    expect(gooseInFront('win32', true, false, [onScreen])).toBe(false);
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
