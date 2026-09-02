import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import SessionActionsHeader from './SessionActionsHeader';
import { IntlTestWrapper } from '../i18n/test-utils';
import type { Session } from '../types/session';
import { assertStudioClean } from './lz/assertStudioClean';
import { missingUtilities } from './lz/compileStudioCss';
import { acpExportSession } from '../acp/sessions';

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

  it('sets the title in the Studio h2 step and carries no ban', () => {
    const { container } = mount(makeSession({ name: 'MLX confirmation' }));
    const title = screen.getByText('MLX confirmation');
    expect(title.className).toContain('truncate');
    expect(title.className).toContain('text-lz-h2');
    expect(title.className).toContain('text-lz-ink');
    expect(title.className).not.toMatch(/font-medium|text-sm/);
    assertStudioClean(container);
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

/**
 * The JSON viewer's values are coloured by the Studio syntax palette (main.css
 * --color-lz-syntax-{key,string,number,bool}, solid in both themes), not by hand-written Tailwind
 * palette classes. The indent guide under an open container stays: it is a structural guide
 * (border-l border-lz-border), not an accent rail.
 */
describe('SessionActionsHeader — the JSON viewer wears the Studio syntax palette', () => {
  it('keys, strings, numbers and booleans carry the syntax tokens; no palette colour survives', async () => {
    vi.mocked(acpExportSession).mockResolvedValueOnce(
      JSON.stringify({ name: 'hello', count: 42, live: true, long: 'x'.repeat(2000) })
    );
    mount(makeSession({ name: 'Palette' }));
    fireEvent.pointerDown(
      screen.getByTestId('session-title-trigger'),
      new PointerEvent('pointerdown', { bubbles: true, button: 0 })
    );
    fireEvent.click(screen.getByTestId('session-title-trigger'));
    fireEvent.click(await screen.findByText('View session JSON'));

    const hello = await screen.findByText('"hello"');
    expect(hello.className).toContain('text-lz-syntax-string');
    expect(screen.getByText('"name":').className).toContain('text-lz-syntax-key');
    expect(screen.getByText('42').className).toContain('text-lz-syntax-number');
    expect(screen.getByText('true').className).toContain('text-lz-syntax-bool');
    const longString = screen.getByTitle('root.long');
    expect(longString.tagName).toBe('BUTTON');
    expect(longString.className).toContain('text-lz-syntax-string');
    expect(longString.className).toContain('decoration-dotted');

    const dialog = hello.closest<HTMLElement>('[role="dialog"]')!;
    expect(dialog.innerHTML).not.toMatch(/text-(blue|emerald|purple|amber)-\d|dark:text-/);
    expect(dialog.querySelector('.border-l.border-lz-border')).not.toBeNull();
    expect(
      await missingUtilities([
        'text-lz-syntax-key',
        'text-lz-syntax-string',
        'text-lz-syntax-number',
        'text-lz-syntax-bool',
        'decoration-dotted',
        'hover:decoration-solid',
      ])
    ).toEqual([]);
  }, 30_000);
});
