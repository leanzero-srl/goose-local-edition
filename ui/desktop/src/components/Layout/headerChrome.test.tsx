import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { useState } from 'react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import SessionActionsHeader from '../SessionActionsHeader';
import type { Session } from '../../types/session';
import { headerInsets, useHeaderObstacle } from './headerChrome';
import { missingUtilities } from '../lz/compileStudioCss';

vi.mock('../../acp/sessions', () => ({
  acpExportSession: vi.fn(async () => '{}'),
  acpForkSession: vi.fn(async () => ({})),
  acpRenameSession: vi.fn(async () => ({})),
}));
vi.mock('../recipes/CreateEditRecipeModal', () => ({ default: () => null }));
vi.mock('../../recipe/recipe_management', () => ({ createRecipeFromSession: vi.fn() }));

/**
 * Q-315, the critic's 460 px window with the sidebar collapsed: the toggle + "1 needs you" end at
 * x 245, the brand chip starts at x 336, and the title band spans the chat, 0..460, rows 14..50.
 */
const BAND = { left: 0, right: 460, top: 14, bottom: 50 };
const LEFT_CLUSTER = { left: 102, right: 245, top: 14, bottom: 42 };
const BRAND = { left: 336, right: 444, top: 8, bottom: 40 };

describe('headerInsets — how far the floating chrome reaches into the title band', () => {
  it('the 460 px window: 245 px from the left, 124 px from the right', () => {
    expect(
      headerInsets(BAND, [
        { side: 'left', box: LEFT_CLUSTER },
        { side: 'right', box: BRAND },
      ])
    ).toEqual({ left: 245, right: 124 });
  });

  it('the cluster over an open sidebar (left of the chat) reaches nothing', () => {
    const chat = { ...BAND, left: 300, right: 1400 };
    expect(headerInsets(chat, [{ side: 'left', box: LEFT_CLUSTER }])).toEqual({
      left: 0,
      right: 0,
    });
  });

  it('a hidden chat’s brand chip (display:none — an empty box) and chrome on other rows reach nothing', () => {
    expect(
      headerInsets(BAND, [
        { side: 'right', box: { left: 0, right: 0, top: 0, bottom: 0 } },
        { side: 'left', box: { left: 0, right: 300, top: 60, bottom: 90 } },
      ])
    ).toEqual({ left: 0, right: 0 });
  });
});

function makeSession(): Session {
  return {
    id: 'sess-1',
    name: 'Jira Migration Assessment',
    message_count: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    working_dir: '/tmp',
    extension_data: { active: [], installed: [] },
  } as Session;
}

function Obstacle({ side, box }: { side: 'left' | 'right'; box: typeof BAND }) {
  const [el, setEl] = useState<HTMLDivElement | null>(null);
  useHeaderObstacle(el, side);
  return (
    <div
      ref={(node) => {
        if (node)
          node.getBoundingClientRect = () =>
            ({ ...box, x: box.left, y: box.top }) as ReturnType<Element['getBoundingClientRect']>;
        setEl(node);
      }}
    />
  );
}

describe('the session title keeps clear of the chrome beside it (Q-315)', () => {
  it('its side columns start past the pill and before the brand chip — centred only where it fits', async () => {
    const rect = Element.prototype.getBoundingClientRect;
    Element.prototype.getBoundingClientRect = function (this: Element) {
      if ((this as HTMLElement).dataset?.testid === 'session-title-band') {
        return { ...BAND, x: 0, y: 14, width: 460, height: 36 } as ReturnType<
          Element['getBoundingClientRect']
        >;
      }
      return rect.call(this);
    };
    try {
      render(
        <IntlTestWrapper>
          <Obstacle side="left" box={LEFT_CLUSTER} />
          <Obstacle side="right" box={BRAND} />
          <SessionActionsHeader session={makeSession()} active={false} onSessionChange={vi.fn()} />
        </IntlTestWrapper>
      );
      const band = screen.getByTestId('session-title-band');
      expect(band.style.gridTemplateColumns).toBe(
        'minmax(253px, 1fr) minmax(0, max-content) minmax(132px, 1fr)'
      );
      // Only the title takes clicks and leaves the drag region; the band itself does neither.
      expect(band.className).toContain('pointer-events-none');
      expect(band.className).not.toContain('no-drag');
      const trigger = screen.getByTestId('session-title-trigger');
      expect(trigger.className).toContain('pointer-events-auto');
      expect(trigger.className).toContain('no-drag');
      expect(trigger.className).toContain('min-w-0');
      // `no-drag` is the app's own region class (main.css), not a utility. The trigger's
      // `focus-visible:ring-border-active` compiles to nothing at HEAD too — not this change's.
      const classes = [...band.className.split(/\s+/), ...trigger.className.split(/\s+/)].filter(
        (c) => c.length > 0 && c !== 'no-drag' && c !== 'focus-visible:ring-border-active'
      );
      expect(await missingUtilities(classes)).toEqual([]);
    } finally {
      Element.prototype.getBoundingClientRect = rect;
    }
  }, 30_000);
});
