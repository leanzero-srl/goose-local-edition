import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import { MemoryRouter } from 'react-router-dom';

vi.mock('../../acp/needsYou', () => ({
  acpSessionActivity: vi.fn().mockResolvedValue({ running: [], needsYou: [], failed: [] }),
  acpResolveNeedsYou: vi.fn(),
}));

import SessionActivityIndicator from './SessionActivityIndicator';
import { resetSessionActivityForTests, seedSessionActivityForTests } from './sessionActivityStore';
import { utilityCss } from '../lz/compileStudioCss';

/**
 * The `display` an element's classes give it at a window width, from the rules the real Tailwind
 * pipeline compiles them to: unconditional rules first, then each `@media (width < N)` /
 * `(width >= N)` rule that holds at that width (Tailwind emits variants after the base utility).
 */
async function displayAt(element: HTMLElement, width: number): Promise<string | undefined> {
  const classes = [...element.classList];
  const rules = await utilityCss(classes);
  let display: string | undefined;
  const applicable = rules
    .filter((css): css is string => css != null && /display:/.test(css))
    .map((css) => {
      const below = /@media \(width < (\d+)px\)/.exec(css);
      const atLeast = /@media \(width >= (\d+)px\)/.exec(css);
      const holds = below ? width < Number(below[1]) : atLeast ? width >= Number(atLeast[1]) : true;
      return {
        conditional: Boolean(below || atLeast),
        holds,
        value: /display: ([a-z-]+)/.exec(css)![1],
      };
    });
  for (const rule of applicable.filter((r) => !r.conditional)) display = rule.value;
  for (const rule of applicable.filter((r) => r.conditional && r.holds)) display = rule.value;
  return display;
}

describe('Q-324: the needs-you pill gives the chat title its room at 460 px', () => {
  afterEach(() => resetSessionActivityForTests());

  it('at 460 px the pill is its icon and count; wide, it says the words; its name is always the words', async () => {
    seedSessionActivityForTests({
      needsYou: [
        {
          id: 'ny_1',
          sessionId: 'jira',
          sessionName: 'Jira Migration Assessment',
          workingDir: '/Users/me/mihaiperdum',
          question: 'An inactive project lead — which decision does the script give them?',
          why: 'w',
          recommendedAnswer: 'migrate',
          options: [],
          createdAt: '2026-09-28T09:08:00Z',
          status: 'open',
        },
      ],
    });
    render(
      <IntlProvider locale="en" messages={{}}>
        <MemoryRouter>
          <SessionActivityIndicator />
        </MemoryRouter>
      </IntlProvider>
    );
    const pill = screen.getByTestId('indicator-needs-you');
    expect(pill.getAttribute('aria-label')).toBe('1 needs you');
    const words = screen.getByTestId('indicator-needs-you-words');
    const count = screen.getByTestId('indicator-needs-you-count');
    expect(words.textContent).toBe('1 needs you');
    expect(count.textContent).toBe('1');
    expect(count.getAttribute('aria-hidden')).toBe('true');

    expect(await displayAt(words, 460)).toBe('none');
    expect(await displayAt(count, 460)).toBe('inline');
    expect(await displayAt(words, 1280)).toBeUndefined();
    expect(await displayAt(count, 1280)).toBe('none');
  }, 30_000);
});
