import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { IntlTestWrapper } from './i18n/test-utils';
import { EngineGlanceDesktopRoot } from './components/engineGlance/EngineGlanceDesktopRoot';
import { resetEngineGlanceForTests } from './components/engineGlance/glanceStore';
import {
  EngineGlanceDesktop,
  type GlancePipAction,
  type GlanceWindowPort,
} from './engineGlanceDesktop';
import type { GlancePrefs, GlancePush } from './utils/engineGlance';
import {
  CHAT_ROW,
  SPLIT_READING_BODY,
  figure,
  glancePush,
  measuredRead,
  runningSnapshot,
} from './utils/engineGlance.fixtures';
import { attributeServing } from './utils/mlxServing';
import { toMlxDistributedReport } from './utils/mlxDistributedReport';
import { FLASH_MODEL, FLASH_READY } from './components/leanzero-swarm/mlxDistributed.fixtures';

/**
 * Q-426, the owner: "if i press on any of its icons it will still bring up the main screen and this
 * should not happen". Every control of the floating window, clicked in its real page
 * (EngineGlanceDesktopRoot) and carried by its real IPC action into main's real controller
 * (EngineGlanceDesktop). A goose window comes forward ONLY through the controller's `openEngine` /
 * `openSession` deps — main's two `app.focus({ steal: true })` sites — so recording those two IS
 * recording "raised the main window".
 *
 * The table is exhaustive by construction: a button the page renders that has no row here fails,
 * so a new control cannot ship without saying whether it may bring goose forward.
 */

type Raise = 'none' | 'engine' | `session:${string}`;

const QUESTION = { sessionId: 'q1', sessionName: 'Deploy the site', question: 'Staging?' };

/** Every control the floating window can show, and what a click on it may raise. */
const EXPECTED: Record<string, Raise> = {
  // Its purpose is to open goose there: these, and only these, bring a goose window forward.
  'engine-glance-open': 'engine',
  'engine-glance-chat': `session:${CHAT_ROW.sessionId}`,
  'engine-glance-needs-you': 'session:q1',
  // Everything else acts inside the floating window.
  'engine-glance-details-toggle': 'none',
  'engine-glance-collapse': 'none',
  'engine-glance-expand': 'none',
  'engine-glance-close': 'none',
  'engine-glance-hint-dismiss': 'none',
};

/** Reading the split's prompt for a chat, a question waiting, ranges to show: every control up. */
function everything(prefs: Partial<GlancePrefs> = {}): GlancePush {
  return glancePush(
    runningSnapshot(SPLIT_READING_BODY, {
      engine: 'distributed',
      modelId: FLASH_MODEL,
      serving: attributeServing([CHAT_ROW], 1, [], null),
      measured: measuredRead({ writing: figure(31.2, 29.0, 33.5, 6) }),
    }),
    { distributed: { report: toMlxDistributedReport(FLASH_READY), ageMs: 0 } },
    { running: 1, needsYou: [QUESTION] },
    prefs
  );
}

function portState() {
  const state = { exists: false, visible: false, calls: [] as string[] };
  const port: GlanceWindowPort = {
    ensure: () => {
      state.exists = true;
      state.calls.push('ensure');
    },
    destroy: () => {
      state.exists = false;
      state.visible = false;
      state.calls.push('destroy');
    },
    exists: () => state.exists,
    showInactive: () => {
      state.visible = true;
      state.calls.push('showInactive');
    },
    hide: () => {
      state.visible = false;
      state.calls.push('hide');
    },
    isVisible: () => state.visible,
    getBounds: () => ({ x: 0, y: 0, width: 300, height: 180 }),
    setBounds: () => undefined,
    send: () => undefined,
    displays: () => [{ id: 1, workArea: { x: 0, y: 25, width: 1512, height: 920 } }],
    displayMatching: () => ({ id: 1, workArea: { x: 0, y: 25, width: 1512, height: 920 } }),
    cursorPoint: () => ({ x: 700, y: 400 }),
  };
  return { port, state };
}

const electron = window.electron as unknown as Record<string, unknown>;
const saved: Record<string, unknown> = {};

// jsdom has no pointer capture; the drag hook takes it on press, as Chromium does.
const CAPTURE = ['setPointerCapture', 'releasePointerCapture', 'hasPointerCapture'] as const;
const element = HTMLElement.prototype as unknown as Record<string, unknown>;

beforeEach(() => {
  for (const key of ['on', 'off', 'engineGlancePip', 'engineGlancePrefsSet', 'engineGlanceRead']) {
    saved[key] = electron[key];
  }
  for (const key of CAPTURE) element[key] = key === 'hasPointerCapture' ? () => false : () => {};
});

afterEach(() => {
  resetEngineGlanceForTests(null);
  Object.assign(electron, saved);
  for (const key of CAPTURE) delete element[key];
});

/** The floating window's page wired to main's controller, the way preload + ipcMain wire them. */
function floating(push: GlancePush) {
  const raised: Raise[] = [];
  const actions: GlancePipAction['type'][] = [];
  const { port, state } = portState();
  const desktop = new EngineGlanceDesktop({
    port,
    platform: 'darwin',
    // goose is behind another app: the floating window is up, the case the owner was in.
    gooseWindows: () => [
      {
        onScreen: false,
        focused: false,
        bounds: { x: 400, y: 200, width: 600, height: 500 },
        visibleShare: null,
      },
    ],
    coverage: { measure: () => undefined, forget: () => undefined },
    savePrefs: () => undefined,
    openEngine: () => raised.push('engine'),
    openSession: (sessionId) => raised.push(`session:${sessionId}`),
    tellDismissed: () => false,
    dismissedChanged: () => undefined,
  });
  desktop.update(push);
  desktop.handle({ type: 'size', width: 300, height: 180 });
  Object.assign(electron, {
    on: () => undefined,
    off: () => undefined,
    engineGlanceRead: undefined,
    engineGlancePrefsSet: async () => undefined,
    engineGlancePip: (action: GlancePipAction) => {
      if (action.type !== 'size') actions.push(action.type);
      desktop.handle(action);
    },
  });
  resetEngineGlanceForTests(push);
  render(
    <IntlTestWrapper>
      <EngineGlanceDesktopRoot />
    </IntlTestWrapper>
  );
  expect(state.visible).toBe(true);
  return { raised, actions, state, desktop };
}

function buttonIds(): string[] {
  return [...document.querySelectorAll('button')].map((b) => b.getAttribute('data-testid') ?? '');
}

const CARD_IDS = [
  'engine-glance-open',
  'engine-glance-details-toggle',
  'engine-glance-collapse',
  'engine-glance-close',
  'engine-glance-chat',
  'engine-glance-needs-you',
  'engine-glance-hint-dismiss',
];
const PILL_IDS = ['engine-glance-expand', 'engine-glance-close'];

describe('Q-426: which clicks on the floating window bring goose forward', () => {
  it('the card: every control it renders has a row — nothing unaccounted for', () => {
    floating(everything());
    expect(buttonIds().sort()).toEqual([...CARD_IDS].sort());
    for (const id of buttonIds()) expect(EXPECTED).toHaveProperty([id]);
  });

  it('the pill: every control it renders has a row', () => {
    floating(everything({ desktopCollapsed: true, desktopHintSeen: true }));
    expect(buttonIds().sort()).toEqual([...PILL_IDS].sort());
  });

  for (const id of CARD_IDS) {
    it(`card · ${id}: raises ${EXPECTED[id]}`, () => {
      const { raised } = floating(everything());
      fireEvent.click(screen.getByTestId(id));
      expect(raised).toEqual(EXPECTED[id] === 'none' ? [] : [EXPECTED[id]]);
    });
  }

  for (const id of PILL_IDS) {
    it(`pill · ${id}: raises ${EXPECTED[id]}`, () => {
      const { raised } = floating(everything({ desktopCollapsed: true, desktopHintSeen: true }));
      fireEvent.click(screen.getByTestId(id));
      expect(raised).toEqual([]);
    });
  }

  for (const collapsed of [false, true]) {
    it(`${collapsed ? 'pill' : 'card'} · its body: a press and a click open nothing`, () => {
      const { raised, actions } = floating(
        everything({ desktopCollapsed: collapsed, desktopHintSeen: true })
      );
      const body = screen.getByTestId('engine-glance-body');
      fireEvent.pointerDown(body, { button: 0, pointerId: 1, screenX: 10, screenY: 10 });
      fireEvent.pointerUp(body, { button: 0, pointerId: 1, screenX: 10, screenY: 10 });
      fireEvent.click(body);
      // The words on the card sit on the body: a click there is a click on the body.
      fireEvent.click(screen.getByTestId('engine-glance'));
      expect(raised).toEqual([]);
      expect(actions).not.toContain('open-engine');
      expect(actions).not.toContain('open-session');
    });
  }

  it('the X: gone at once and for the session — the next glances bring nothing back', () => {
    const { raised, actions, state, desktop } = floating(everything({ desktopHintSeen: true }));
    fireEvent.click(screen.getByTestId('engine-glance-close'));
    expect(actions).toEqual(['close']);
    expect(raised).toEqual([]);
    expect(state.exists).toBe(false);
    desktop.update(everything({ desktopHintSeen: true }));
    expect(state.exists).toBe(false);
    expect(desktop.isDismissed()).toBe(true);
  });
});
