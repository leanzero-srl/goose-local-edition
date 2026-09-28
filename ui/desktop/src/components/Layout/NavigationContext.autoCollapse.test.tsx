import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { NavigationProvider, useNavigationContext } from './NavigationContext';

/**
 * Q-321 (3.0.68 critic): narrowed to 460 px the sidebar collapsed — and stayed collapsed when the
 * width came back. The narrowing's collapse was written as if the person had chosen it. Now a
 * sidebar the narrowing closed opens again with the width; one the person closed stays closed.
 */
function Probe() {
  const { isNavExpanded, setIsNavExpanded } = useNavigationContext();
  return (
    <button data-testid="toggle" onClick={() => setIsNavExpanded(!isNavExpanded)}>
      {isNavExpanded ? 'open' : 'closed'}
    </button>
  );
}

const resizeTo = (width: number) =>
  act(() => {
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: width });
    window.dispatchEvent(new Event('resize'));
  });

const state = () => screen.getByTestId('toggle').textContent;

describe('the sidebar through a narrowing and back (Q-321)', () => {
  let width: number;
  beforeEach(() => {
    width = window.innerWidth;
    localStorage.clear();
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1400 });
  });
  afterEach(() => {
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: width });
    localStorage.clear();
  });

  it('closed by the narrowing: open again when the width returns, and nothing persisted', () => {
    render(
      <NavigationProvider>
        <Probe />
      </NavigationProvider>
    );
    expect(state()).toBe('open');
    resizeTo(460);
    expect(state()).toBe('closed');
    expect(localStorage.getItem('navigation_expanded')).toBeNull();
    resizeTo(1000);
    expect(state()).toBe('open');
  });

  it('closed by the person: stays closed through a narrowing and back', () => {
    render(
      <NavigationProvider>
        <Probe />
      </NavigationProvider>
    );
    fireEvent.click(screen.getByTestId('toggle'));
    expect(state()).toBe('closed');
    resizeTo(460);
    resizeTo(1400);
    expect(state()).toBe('closed');
    expect(localStorage.getItem('navigation_expanded')).toBe('false');
  });

  it('the person closes it while narrow: the width’s return leaves it closed', () => {
    render(
      <NavigationProvider>
        <Probe />
      </NavigationProvider>
    );
    resizeTo(460);
    fireEvent.click(screen.getByTestId('toggle'));
    expect(state()).toBe('open');
    fireEvent.click(screen.getByTestId('toggle'));
    expect(state()).toBe('closed');
    resizeTo(1400);
    expect(state()).toBe('closed');
  });

  it('launched narrow: collapsed for the width, open once it widens — the stored choice was open', () => {
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 460 });
    render(
      <NavigationProvider>
        <Probe />
      </NavigationProvider>
    );
    expect(state()).toBe('closed');
    resizeTo(1400);
    expect(state()).toBe('open');
  });
});
