import { describe, expect, it } from 'vitest';
import {
  cornerBounds,
  desktopGlanceVisible,
  dockFits,
  glanceHasContent,
  glanceLive,
  inAppPlacement,
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

describe('inAppPlacement — docked in the sidebar or floating over the window', () => {
  it('the sidebar with room docks it, idle included', () => {
    expect(inAppPlacement(idle, { navExpanded: true, dockRoom: true })).toBe('dock');
    expect(inAppPlacement(writing, { navExpanded: true, dockRoom: true })).toBe('dock');
  });

  it('no room (or no sidebar): it floats only while live', () => {
    expect(inAppPlacement(writing, { navExpanded: true, dockRoom: false })).toBe('float');
    expect(inAppPlacement(writing, { navExpanded: false, dockRoom: true })).toBe('float');
    expect(inAppPlacement(idle, { navExpanded: false, dockRoom: false })).toBe('hidden');
  });

  it('turned off, or nothing to show: hidden', () => {
    const offPref = { ...writing, prefs: { ...writing.prefs, inApp: false } };
    expect(inAppPlacement(offPref, { navExpanded: true, dockRoom: true })).toBe('hidden');
    expect(inAppPlacement(off, { navExpanded: true, dockRoom: true })).toBe('hidden');
  });
});

describe('the geometry', () => {
  it('dockFits: the trees plus the card must fit the column; an unmeasured card never docks', () => {
    expect(dockFits(800, 500, 140)).toBe(true);
    expect(dockFits(800, 700, 140)).toBe(false);
    expect(dockFits(800, 100, 0)).toBe(false);
  });

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
