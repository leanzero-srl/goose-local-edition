import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import { NavRow } from './NavigationPanel';

/**
 * The active nav row must be identifiable WITHOUT reading its colours.
 *
 * It was styled-only — `bg-background-tertiary` against a transparent sibling and no ARIA state at all —
 * so the current view was visible to a sighted mouse user and to nothing else. Found by probing the
 * RUNNING app over CDP rather than by reading the code: on #/benchmark the Benchmark row computed
 * rgb(71,78,87) against a sibling's rgba(0,0,0,0), while a sweep of all 19 nav controls found zero with
 * aria-current or aria-selected.
 */
const Icon = () => <svg data-testid="icon" />;
const item = (label: string) => ({ key: label.toLowerCase(), icon: Icon, label }) as never;

const renderRows = () =>
  render(
    <IntlProvider locale="en" messages={{}}>
      <NavRow item={item('Benchmark')} active onClick={() => {}} />
      <NavRow item={item('Recipes')} active={false} onClick={() => {}} />
    </IntlProvider>
  );

describe('NavigationPanel active state', () => {
  it('marks exactly one row as the current page', () => {
    renderRows();
    const marked = screen
      .getAllByRole('button')
      .filter((b) => b.getAttribute('aria-current') === 'page');
    expect(marked).toHaveLength(1);
    expect(marked[0].textContent).toContain('Benchmark');
  });

  it('leaves inactive rows with no aria-current at all, not aria-current="false"', () => {
    renderRows();
    const inactive = screen
      .getAllByRole('button')
      .filter((b) => (b.textContent ?? '').includes('Recipes'));
    expect(inactive).toHaveLength(1);
    expect(inactive[0].hasAttribute('aria-current')).toBe(false);
  });
});
