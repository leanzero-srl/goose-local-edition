import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import type { FormingStatus } from '@aaif/goose-sdk';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { EngineGlanceDockSlot } from './EngineGlanceInApp';
import { glanceSessionsOf, resetEngineGlanceForTests } from './glanceStore';
import { CHAT_ROW, glancePush, runningSnapshot } from '../../utils/engineGlance.fixtures';
import { attributeServing } from '../../utils/mlxServing';
import { INITIAL_SNAPSHOT } from '../../utils/mlxEngineMonitor';
import type { GlancePrefs, GlancePush } from '../../utils/engineGlance';
import { GENERATING_STATUS, IDLE_STATUS } from '../leanzero-swarm/mlxLiveStatus.fixtures';
import { resetFormingForTests, usePublishForming } from '../forming/formingStore';

function Where() {
  const location = useLocation();
  return <span data-testid="where">{`${location.pathname}${location.search}`}</span>;
}

function PublishForming({ sessionId, forming }: { sessionId: string; forming: FormingStatus }) {
  usePublishForming(sessionId, forming);
  return null;
}

const prefsSet = vi.fn(async (_prefs: GlancePrefs) => undefined);

function renderDock(push: GlancePush, extra: React.ReactNode = null) {
  resetEngineGlanceForTests(push);
  Object.assign(window.electron as unknown as Record<string, unknown>, {
    engineGlancePrefsSet: prefsSet,
  });
  render(
    <IntlTestWrapper>
      <MemoryRouter initialEntries={['/']}>
        <Routes>
          <Route
            path="*"
            element={
              <>
                {extra}
                <EngineGlanceDockSlot />
                <Where />
              </>
            }
          />
        </Routes>
      </MemoryRouter>
    </IntlTestWrapper>
  );
}

afterEach(() => {
  resetEngineGlanceForTests(null);
  resetFormingForTests();
  prefsSet.mockClear();
});

const writing = glancePush(
  runningSnapshot(GENERATING_STATUS, { serving: attributeServing([CHAT_ROW], 3, [], null) })
);

describe('EngineGlanceDockSlot — the card at the foot of the sidebar', () => {
  it('writing: the docked card, idle or not — no room is measured and nothing floats', () => {
    renderDock(writing);
    expect(screen.getByTestId('engine-glance').dataset.variant).toBe('dock');
    expect(screen.getByTestId('engine-glance-dock').className).toContain('overflow-y-auto');
  });

  it('idle: still docked (the quiet grey card)', () => {
    renderDock(glancePush(runningSnapshot(IDLE_STATUS)));
    expect(screen.getByTestId('engine-glance').dataset.stage).toBe('idle');
  });

  it('a click opens the Engine tab by name', () => {
    renderDock(writing);
    fireEvent.click(screen.getByTestId('engine-glance-open'));
    expect(screen.getByTestId('where').textContent).toBe('/leanzero-swarm?tab=mlx');
  });

  it('Q-218: hide stores inApp false for this person (main writes settings.json)', () => {
    renderDock(writing);
    fireEvent.click(screen.getByTestId('engine-glance-hide'));
    expect(prefsSet).toHaveBeenCalledWith({ ...writing.prefs, inApp: false });
    expect(screen.getByTestId('where').textContent).toBe('/');
  });

  it('Q-218: hidden, one row in its place says what the engine does and brings the card back', () => {
    renderDock({ ...writing, prefs: { ...writing.prefs, inApp: false } });
    expect(screen.queryByTestId('engine-glance')).toBeNull();
    const restore = screen.getByTestId('engine-glance-restore');
    expect(restore.textContent).toContain('Show the engine card');
    const stage = screen.getByTestId('engine-glance-restore-stage');
    expect(stage.textContent).toBe('Writing');
    // The engine's solid phase fill, never a tint.
    expect(stage.className).toContain('bg-lz-phase-writing');
    fireEvent.click(restore);
    expect(prefsSet).toHaveBeenCalledWith({ ...writing.prefs, inApp: true });
  });

  it('hidden with nothing to show: no card and no row (restoring an empty card would read as broken)', () => {
    const off = glancePush({ ...INITIAL_SNAPSHOT, mode: 'off' }, {}, undefined, { inApp: false });
    renderDock(off);
    expect(screen.queryByTestId('engine-glance')).toBeNull();
    expect(screen.queryByTestId('engine-glance-restore')).toBeNull();
  });

  it('Q-215: the chat the card serves is forming calls — "What it’s writing" is on the card', () => {
    const forming: FormingStatus = {
      calls: [{ name: 'developer__text_editor', title: 'edit', argumentChars: 608 }],
      argumentChars: 608,
      reasoningChars: 0,
      text: '',
    };
    renderDock(
      writing,
      <PublishForming sessionId={CHAT_ROW.sessionId as string} forming={forming} />
    );
    const toggle = screen.getByTestId('engine-glance-forming-toggle');
    act(() => {
      fireEvent.click(toggle);
    });
    expect(screen.getByTestId('forming-panel').textContent).toContain('edit');
  });

  it('negative control: another chat forming calls is not offered on this card', () => {
    const forming: FormingStatus = {
      calls: [{ name: 'developer__shell', title: 'shell', argumentChars: 20 }],
      argumentChars: 20,
      reasoningChars: 0,
      text: '',
    };
    renderDock(writing, <PublishForming sessionId="some-other-chat" forming={forming} />);
    expect(screen.queryByTestId('engine-glance-forming-toggle')).toBeNull();
  });
});

describe('glanceSessionsOf — the session-state store as the glance reports it', () => {
  it('counts running turns and lists every open question, with its session', () => {
    const report = glanceSessionsOf({
      running: [
        {
          sessionId: 'a',
          sessionName: 'Auth',
          workingDir: '/p',
          startedAt: '2026-09-27T10:00:00Z',
        },
      ],
      needsYou: [
        {
          id: 'n1',
          sessionId: 'b',
          sessionName: 'Deploy',
          workingDir: '/p',
          question: 'Staging or prod?',
          createdAt: '2026-09-27T10:01:00Z',
        },
      ],
      failed: [],
      stopped: [],
      notesWaiting: [],
      elicitations: [],
    } as unknown as Parameters<typeof glanceSessionsOf>[0]);
    expect(report).toEqual({
      running: 1,
      needsYou: [{ sessionId: 'b', sessionName: 'Deploy', question: 'Staging or prod?' }],
    });
  });
});
