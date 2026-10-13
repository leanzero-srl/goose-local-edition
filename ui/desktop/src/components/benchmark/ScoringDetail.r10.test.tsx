import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ScoringDetail, type VerdictDetail } from './ScoringDetail';
import native from './forge2-r10-native.fixture.json';

// Exact R10 excerpt of Opus build one's retained canonical 105-check verdict; custody hash is in the fixture.
describe('qualified Forge Native JQL evidence vocabulary', () => {
  it('renders the actual R10 weight, mean and all six native check rows without fabricating tiers', () => {
    render(
      <ScoringDetail
        verdict={native as unknown as VerdictDetail}
        scorerVersion="forge-2.0"
        score={native.score}
      />
    );
    const row = screen.getByTestId('forge-group-line');
    expect(row).toHaveAttribute('data-tier', 'R10');
    expect([...row.querySelectorAll('td')].map((cell) => cell.textContent)).toEqual([
      'R10 Native JQL',
      '0.9120',
      '10%',
      '0.0912',
    ]);
    const group = screen.getByRole('button', { name: /R10.*Native JQL.*6 tests/ });
    if (group.getAttribute('aria-expanded') !== 'true') fireEvent.click(group);
    const evidence = group.parentElement!;
    expect(within(evidence).getAllByTestId('score-chip')).toHaveLength(native.checks.length + 1);
    for (const check of native.checks) {
      expect(within(evidence).getByText(check.detail)).toBeVisible();
      const label = check.check.replace(/_/g, ' ');
      const checkRow = within(evidence).getByText(label[0].toUpperCase() + label.slice(1))
        .parentElement!.parentElement!.parentElement!;
      expect(within(checkRow).getByTestId('score-chip')).toHaveTextContent(check.score.toFixed(4));
    }
  });
});
