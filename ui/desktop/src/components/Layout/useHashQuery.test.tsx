import { act, render, screen } from '@testing-library/react';
import { MemoryRouter, useNavigate } from 'react-router-dom';
import { afterEach, expect, it } from 'vitest';
import { useHashQuery } from './useHashQuery';

function Probe() {
  const query = useHashQuery();
  const navigate = useNavigate();
  return (
    <>
      <span data-testid="run">{query.get('run') ?? 'none'}</span>
      <button onClick={() => navigate('/benchmark?era=sb-7.2&run=b')}>go</button>
    </>
  );
}

afterEach(() => {
  window.location.hash = '';
});

/** MEASURED 2026-10-03: the sidebar's navigate() is a pushState — no hashchange, no popstate — so the
 *  window-event reader kept showing the previous run until reload. Inside a router the router decides. */
it('follows a router navigation that fires no window event', () => {
  render(
    <MemoryRouter initialEntries={['/benchmark?era=sb-7.2&run=a']}>
      <Probe />
    </MemoryRouter>
  );
  expect(screen.getByTestId('run')).toHaveTextContent('a');
  act(() => screen.getByText('go').click());
  expect(screen.getByTestId('run')).toHaveTextContent('b');
});

it('reads the window hash outside a router (bare view mounts) and follows hashchange', () => {
  function Bare() {
    return <span data-testid="run">{useHashQuery().get('run') ?? 'none'}</span>;
  }
  window.location.hash = '#/benchmark?run=x';
  render(<Bare />);
  expect(screen.getByTestId('run')).toHaveTextContent('x');
  act(() => {
    window.location.hash = '#/benchmark?run=y';
    window.dispatchEvent(new Event('hashchange'));
  });
  expect(screen.getByTestId('run')).toHaveTextContent('y');
});
