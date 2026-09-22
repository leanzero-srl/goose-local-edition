import { fireEvent, render, screen } from '@testing-library/react';
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
    for (const c of [
      'border',
      'border-lz-border',
      'rounded-lz-card',
      'overflow-hidden',
      'h-full',
      'w-full',
    ]) {
      expect(frame.className).toContain(c);
    }
    expect(frame.className).not.toMatch(/rounded-xl|border-border-primary|shadow/);
    expect(frame.contains(screen.getByTestId('nav'))).toBe(true);
    assertStudioClean(frame);
    expect(await missingUtilities(['border-lz-border', 'rounded-lz-card'])).toEqual([]);
  }, 30_000);

  it('the sidebar edge is a drag handle: dragging resizes within the clamp, double-click resets, and the width is remembered', async () => {
    Object.assign(window.electron as unknown as Record<string, unknown>, {
      platform: 'darwin',
      getIsFullScreen: vi.fn(async () => false),
      on: vi.fn(),
      off: vi.fn(),
    });
    try {
      localStorage.removeItem('navigation_width');
    } catch {
      // a blocked store only loses the remembered width
    }
    render(
      <IntlTestWrapper>
        <MemoryRouter>
          <AppLayout activeSessions={[]} />
        </MemoryRouter>
      </IntlTestWrapper>
    );
    const handle = screen.getByTestId('nav-resize-handle');
    expect(handle.getAttribute('aria-valuenow')).toBe('280');
    handle.setPointerCapture = vi.fn();
    fireEvent.pointerDown(handle, { clientX: 280, pointerId: 1 });
    fireEvent.pointerMove(handle, { clientX: 400, pointerId: 1 });
    fireEvent.pointerUp(handle, { clientX: 400, pointerId: 1 });
    expect(handle.getAttribute('aria-valuenow')).toBe('400');
    expect(localStorage.getItem('navigation_width')).toBe('400');
    fireEvent.pointerDown(handle, { clientX: 400, pointerId: 1 });
    fireEvent.pointerMove(handle, { clientX: 2000, pointerId: 1 });
    fireEvent.pointerUp(handle, { clientX: 2000, pointerId: 1 });
    expect(handle.getAttribute('aria-valuenow')).toBe('560');
    fireEvent.doubleClick(handle);
    expect(handle.getAttribute('aria-valuenow')).toBe('280');
    assertStudioClean(handle);
  });
});
