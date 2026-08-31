import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import SessionActionsHeader from './SessionActionsHeader';
import { IntlTestWrapper } from '../i18n/test-utils';
import type { Session } from '../types/session';

/**
 * Pass E — the session header:
 *  - the title trigger is a generous, obviously-clickable target (renaming was hard to hit);
 *  - "Make recipe from this conversation" is gone from the menu (hidden, not deleted);
 *  - the title shows the backend-generated name when one exists, else "New Session".
 */

vi.mock('../acp/sessions', () => ({
  acpExportSession: vi.fn(async () => '{}'),
  acpForkSession: vi.fn(async () => ({})),
  acpRenameSession: vi.fn(async () => ({})),
}));
vi.mock('./recipes/CreateEditRecipeModal', () => ({ default: () => null }));
vi.mock('../recipe/recipe_management', () => ({ createRecipeFromSession: vi.fn() }));

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'New Chat',
    message_count: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    working_dir: '/tmp',
    extension_data: { active: [], installed: [] },
    ...overrides,
  } as Session;
}

const mount = (session: Session) =>
  render(
    <IntlTestWrapper>
      <SessionActionsHeader session={session} onSessionChange={vi.fn()} />
    </IntlTestWrapper>
  );

describe('SessionActionsHeader (pass E)', () => {
  it('titles a fresh session "New Session" on a generous click target', () => {
    mount(makeSession());
    const trigger = screen.getByTestId('session-title-trigger');
    expect(trigger).toHaveTextContent('New Session');
    expect(trigger.className).toContain('cursor-pointer');
    expect(trigger.className).toContain('h-9');
    expect(trigger.className).toContain('px-4');
  });

  it('shows the backend-generated name once the session is auto-named', () => {
    mount(makeSession({ name: 'MLX confirmation' }));
    expect(screen.getByTestId('session-title-trigger')).toHaveTextContent('MLX confirmation');
  });

  it('offers rename but never "Make recipe from this conversation"', async () => {
    mount(makeSession());
    fireEvent.pointerDown(
      screen.getByTestId('session-title-trigger'),
      new PointerEvent('pointerdown', { bubbles: true, button: 0 })
    );
    fireEvent.click(screen.getByTestId('session-title-trigger'));
    await waitFor(() => expect(screen.getByText('Rename session')).toBeInTheDocument());
    expect(screen.getByText('Duplicate session')).toBeInTheDocument();
    expect(screen.getByText('View session JSON')).toBeInTheDocument();
    expect(screen.queryByText('Make recipe from this conversation')).toBeNull();
  });
});
