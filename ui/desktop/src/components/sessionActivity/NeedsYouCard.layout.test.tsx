import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { IntlProvider } from 'react-intl';

vi.mock('../../acp/needsYou', () => ({
  acpSessionActivity: vi.fn(),
  acpResolveNeedsYou: vi.fn(),
}));

import { QuestionCard } from './NeedsYouCard';
import { assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

/**
 * Q-315 (61.png, 460 px): "migrate + special 'lead-inactive' marker so the review list catches
 * exactly 6" ran out of its pill — a fixed h-7 with a 999 radius cannot hold two lines. The options
 * are sentences: the chip grows with its text and keeps the chip radius. (What the options SAY is
 * the needs-you logic's; this is only their box.)
 */
const ITEM = {
  id: 'ny_1',
  sessionId: 's1',
  sessionName: 'Jira Migration Assessment',
  workingDir: '/w',
  question: 'An inactive project lead — which decision does the script give them?',
  why: 'The stated rules conflict on exactly that case.',
  recommendedAnswer: 'migrate — apply the lead override in every case',
  options: [
    'migrate — apply the lead override in every case',
    "migrate + special 'lead-inactive' marker so the review list catches exactly 6",
    "skip unless they have an email — a lead without an email can't be created in Cloud anyway",
  ],
  createdAt: '2026-09-28T08:42:00Z',
  status: 'open' as const,
};

describe('the needs-you options hold their own text (Q-315)', () => {
  it('each option grows in height with its sentence — no fixed height, the chip radius, never past the card', async () => {
    render(
      <IntlProvider locale="en" messages={{}}>
        <QuestionCard
          item={ITEM}
          index={1}
          total={1}
          busy={false}
          onAnswer={vi.fn(async () => undefined)}
          onDismiss={vi.fn(async () => undefined)}
        />
      </IntlProvider>
    );
    const options = screen.getAllByTestId('needs-you-option');
    // Q-319 leaves the recommended answer out of the pick list, so the two other sentences remain.
    expect(options).toHaveLength(2);
    for (const option of options) {
      const classes = option.className.split(/\s+/);
      expect(classes).not.toContain('h-7');
      expect(classes).toEqual(
        expect.arrayContaining([
          'min-h-7',
          'py-1',
          'max-w-full',
          'whitespace-normal',
          'break-words',
          'text-left',
          'rounded-lz-control',
        ])
      );
      expect(classes).not.toContain('rounded-lz-pill');
      assertStudioClean(option);
    }
    expect(await missingUtilities(options[1].className.split(/\s+/))).toEqual([]);
  }, 30_000);
});
