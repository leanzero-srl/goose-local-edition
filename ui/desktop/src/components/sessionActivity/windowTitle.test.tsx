import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import SessionActionsHeader from '../SessionActionsHeader';
import type { Session } from '../../types/session';
import { resetSessionActivityForTests, seedSessionActivityForTests } from './sessionActivityStore';

vi.mock('../../acp/sessions', () => ({
  acpExportSession: vi.fn(async () => '{}'),
  acpForkSession: vi.fn(async () => ({})),
  acpRenameSession: vi.fn(async () => ({})),
}));
vi.mock('../recipes/CreateEditRecipeModal', () => ({ default: () => null }));
vi.mock('../../recipe/recipe_management', () => ({ createRecipeFromSession: vi.fn() }));
vi.mock('../../acp/needsYou', () => ({
  acpSessionActivity: vi.fn(async () => ({ running: [], needsYou: [], failed: [] })),
  acpResolveNeedsYou: vi.fn(),
}));

/**
 * Q-318: the window's title named nothing ("Goose Swarm" throughout a live turn). It now names the
 * chat on screen and its state, from the one session-activity store the sidebar rows read.
 */
const SESSION = {
  id: 'sess-1',
  name: 'Jira Migration Assessment',
  message_count: 3,
  created_at: '2026-09-28T08:00:00Z',
  updated_at: '2026-09-28T08:40:00Z',
  working_dir: '/w',
  extension_data: { active: [], installed: [] },
} as unknown as Session;

const RUNNING = {
  sessionId: 'sess-1',
  sessionName: 'Jira Migration Assessment',
  workingDir: '/w',
  startedAt: '2026-09-28T08:41:00Z',
};

const QUESTION = {
  id: 'ny_1',
  sessionId: 'sess-1',
  sessionName: 'Jira Migration Assessment',
  workingDir: '/w',
  question: 'An inactive project lead — which decision does the script give them?',
  why: 'The stated rules conflict on exactly that case.',
  recommendedAnswer: 'migrate',
  options: [],
  createdAt: '2026-09-28T08:42:00Z',
  status: 'open' as const,
};

const mount = (active: boolean) =>
  render(
    <IntlTestWrapper>
      <SessionActionsHeader session={SESSION} active={active} onSessionChange={vi.fn()} />
    </IntlTestWrapper>
  );

describe('the window title names the chat on screen and what it is doing (Q-318)', () => {
  beforeEach(() => {
    localStorage.setItem('edition', 'local');
    document.title = 'Goose Swarm';
  });
  afterEach(() => resetSessionActivityForTests());

  it('a turn in flight: "<session> — Running", the sidebar pill’s word', () => {
    seedSessionActivityForTests({ running: [RUNNING] });
    mount(true);
    expect(document.title).toBe('Jira Migration Assessment — Running');
  });

  it('a question waiting outranks the turn, as it does on the row', () => {
    seedSessionActivityForTests({ running: [RUNNING], needsYou: [QUESTION] });
    mount(true);
    expect(document.title).toBe('Jira Migration Assessment — Needs you');
  });

  it('the turn ends: the state follows the store, and idle is the name alone', () => {
    seedSessionActivityForTests({ running: [RUNNING] });
    mount(true);
    act(() => seedSessionActivityForTests({}));
    expect(document.title).toBe('Jira Migration Assessment');
    act(() =>
      seedSessionActivityForTests({
        failed: [{ ...RUNNING, failedAt: '2026-09-28T08:50:00Z', reason: 'boom' }],
      })
    );
    expect(document.title).toBe('Jira Migration Assessment — Failed');
  });

  it('a chat kept alive behind another never names the window; leaving the chat gives the brand back', () => {
    seedSessionActivityForTests({ running: [RUNNING] });
    const hidden = mount(false);
    expect(document.title).toBe('Goose Swarm');
    hidden.unmount();
    const shown = mount(true);
    expect(document.title).toBe('Jira Migration Assessment — Running');
    shown.unmount();
    expect(document.title).toBe('Goose Swarm');
  });
});
