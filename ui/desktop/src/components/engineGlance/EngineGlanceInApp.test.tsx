import { afterEach, describe, expect, it } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { EngineGlanceFloat } from './EngineGlanceInApp';
import { glanceSessionsOf, resetEngineGlanceForTests } from './glanceStore';
import { glancePush, runningSnapshot } from '../../utils/engineGlance.fixtures';
import type { GlancePush } from '../../utils/engineGlance';
import { GENERATING_STATUS, IDLE_STATUS } from '../leanzero-swarm/mlxLiveStatus.fixtures';

function Where() {
  const location = useLocation();
  return <span data-testid="where">{`${location.pathname}${location.search}`}</span>;
}

function renderFloat(push: GlancePush, navExpanded = false) {
  resetEngineGlanceForTests(push);
  render(
    <IntlTestWrapper>
      <MemoryRouter initialEntries={['/']}>
        <Routes>
          <Route
            path="*"
            element={
              <div className="relative">
                <EngineGlanceFloat navExpanded={navExpanded} />
                <Where />
              </div>
            }
          />
        </Routes>
      </MemoryRouter>
    </IntlTestWrapper>
  );
}

afterEach(() => {
  resetEngineGlanceForTests(null);
  localStorage.clear();
});

describe('EngineGlanceFloat — over the window when the sidebar has no room', () => {
  it('the sidebar collapsed and the engine writing: it floats in the bottom-right corner', () => {
    renderFloat(glancePush(runningSnapshot(GENERATING_STATUS)));
    const float = screen.getByTestId('engine-glance-float');
    expect(float.dataset.corner).toBe('bottom-right');
    expect(screen.getByTestId('engine-glance').dataset.variant).toBe('float');
  });

  it('an idle engine never floats over the content', () => {
    renderFloat(glancePush(runningSnapshot(IDLE_STATUS)));
    expect(screen.queryByTestId('engine-glance-float')).toBeNull();
  });

  it('turned off in Settings: nothing', () => {
    renderFloat(glancePush(runningSnapshot(GENERATING_STATUS), {}, undefined, { inApp: false }));
    expect(screen.queryByTestId('engine-glance-float')).toBeNull();
  });

  it('a click opens the Engine tab by name', () => {
    renderFloat(glancePush(runningSnapshot(GENERATING_STATUS)));
    fireEvent.click(screen.getByTestId('engine-glance-open'));
    expect(screen.getByTestId('where').textContent).toBe('/leanzero-swarm?tab=mlx');
  });

  it('shrunk to a pill, it stays a pill across a remount (remembered per machine)', () => {
    renderFloat(glancePush(runningSnapshot(GENERATING_STATUS)));
    act(() => {
      fireEvent.click(screen.getByTestId('engine-glance-collapse'));
    });
    expect(screen.getByTestId('engine-glance').dataset.collapsed).toBe('true');
    expect(localStorage.getItem('engineGlance.float.collapsed')).toBe('1');
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
      elicitations: [],
    } as unknown as Parameters<typeof glanceSessionsOf>[0]);
    expect(report).toEqual({
      running: 1,
      needsYou: [{ sessionId: 'b', sessionName: 'Deploy', question: 'Staging or prod?' }],
    });
  });
});
