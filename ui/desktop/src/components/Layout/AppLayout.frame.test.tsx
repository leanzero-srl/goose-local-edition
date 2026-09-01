import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { MemoryRouter } from 'react-router-dom';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { AppLayout } from './AppLayout';
import { assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

vi.mock('./NavigationPanel', () => ({ Navigation: () => <div data-testid="nav" /> }));
vi.mock('../ChatSessionsContainer', () => ({ default: () => null }));
vi.mock('../../contexts/ChatContext', () => ({ useChatContext: () => ({ setChat: vi.fn() }) }));

/** Studio remake: the sidebar's frame is the lz hairline at the card radius. Width, spring, collapse: untouched. */
describe('AppLayout (Studio frame)', () => {
  it('frames the navigation in border-lz-border at rounded-lz-card, never the host rounded-xl outline', async () => {
    Object.assign(window.electron as unknown as Record<string, unknown>, {
      platform: 'darwin',
      getIsFullScreen: vi.fn(async () => false),
      on: vi.fn(),
      off: vi.fn(),
    });
    render(
      <IntlTestWrapper>
        <MemoryRouter>
          <AppLayout activeSessions={[]} />
        </MemoryRouter>
      </IntlTestWrapper>
    );
    const frame = screen.getByTestId('nav-frame');
    for (const c of ['border', 'border-lz-border', 'rounded-lz-card', 'overflow-hidden', 'h-full', 'w-full']) {
      expect(frame.className).toContain(c);
    }
    expect(frame.className).not.toMatch(/rounded-xl|border-border-primary|shadow/);
    expect(frame.contains(screen.getByTestId('nav'))).toBe(true);
    assertStudioClean(frame);
    expect(await missingUtilities(['border-lz-border', 'rounded-lz-card'])).toEqual([]);
  }, 30_000);
});
