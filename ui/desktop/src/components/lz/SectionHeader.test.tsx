import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { SectionHeader } from './SectionHeader';
import { TYPE } from './tokens';
import { assertStudioClean } from './assertStudioClean';

describe('lz/SectionHeader', () => {
  it('is the zone register with a tabular count pill and a right slot — no square, no rail', () => {
    const { container, getByTestId, getByRole } = render(
      <SectionHeader title="Lanes" count={3} right={<span>filter</span>} />
    );
    const h = getByRole('heading', { level: 2 });
    for (const c of TYPE.zone.split(' ')) expect(h.className).toContain(c);
    const pill = getByTestId('lz-section-count');
    expect(pill.textContent).toBe('3');
    expect(pill.className).toContain('tnum');
    expect(pill.className).toContain('rounded-lz-pill');
    expect(pill.className).not.toMatch(/bg-lz-(node|ok|warn|err|accent)/);
    expect(container.querySelector('[data-testid="lz-status-dot"]')).toBeNull();
    assertStudioClean(container);
  });

  it('omits the pill when no count is given and can render as h3', () => {
    const { container, queryByTestId, getByRole } = render(<SectionHeader as="h3" title="Feed" />);
    expect(queryByTestId('lz-section-count')).toBeNull();
    expect(getByRole('heading', { level: 3 }).textContent).toBe('Feed');
    assertStudioClean(container);
  });
});
